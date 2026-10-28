//! Hashline patch parser: `[PATH#TAG]` sections with PUT/CUT/REM/MV hunks and `+` body rows.

use std::sync::LazyLock;

use regex::Regex;

pub const BLOCK_RESOLVER_UNAVAILABLE: &str = "Block locators (`N*` in `PUT N*:`, `PUT >N*`, `CUT N*`) are not available here (no block resolver configured). Use a concrete line range.";
const BARE_BODY_WARNING: &str =
    "Auto-prefixed bare body row(s) with `+`. Body rows must be `+TEXT` literal lines.";
const SNAPSHOT_ROWS_WARNING: &str = "Recovered top-level `N:TEXT` snapshot row(s) as single-line `PUT N.=N:` replacements. Use explicit `PUT` headers for reliable edits.";
const BARE_RANGE_WARNING: &str =
    "Recovered a bare `N.=M:` header as `PUT N.=M:`. Prefix replacement ranges with `PUT`.";
const METADATA_IGNORED_WARNING: &str =
    "Ignored copied read-output elision row(s). Re-read elided ranges before editing them.";
const EMPTY_PUT_WARNING: &str =
    "Interpreted an empty `PUT` body as deletion. Use `CUT N.=M` for bodyless deletes.";
const CUT_COLON_WARNING: &str =
    "Ignored a trailing `:` on bodyless `CUT`. Prefer `CUT N.=M` without a colon.";
const MINUS_BULLET_WARNING: &str = "Auto-prefixed bare `- ` bullet row(s) as literal content. `-` rows never remove lines — the range does that; always prefix literal body rows with `+`: `+- item`.";
const DIFF_OLD_ROWS_WARNING: &str = "Ignored unified-diff `-old` row(s); the range already removes old content, so only `+new` rows were kept.";
const MINUS_ROW_REJECTED: &str = "`-` rows are not valid; the range already names the lines being changed. For Markdown bullets or other literal `-` lines, prefix the literal row with `+`: `+- item`.";
const PREFIX_STRIPPED_WARNING: &str =
    "Stripped copied `N:` read prefixes from bare body rows. Body rows must be `+TEXT`.";
const DUP_RANGE_WARNING: &str = "Multiple hunks targeted the same exact range; kept only the last. Issue one `PUT` or `CUT` hunk per range.";
const REM_TAKES_NO_BODY: &str =
    "`REM` deletes the whole file and takes no body rows or line ops. Issue it alone under the header.";
const MOVE_TAKES_NO_BODY: &str = "`MV DEST` does not take body rows. Put line edits above the `MV` row; the destination path follows `MV` on the same line.";
const CUT_TAKES_NO_BODY: &str = "`CUT` deletes (and captures) the named lines and takes no body rows. To write new content, use `PUT N.=M:` with `+TEXT` rows.";
const COLON_ON_REGISTER_PUT: &str = "`PUT … @name` pastes the register and never takes `:` — the colon promises body rows. Drop the colon (`PUT >40 @name`), or drop `@name` and write `+TEXT` body rows.";
const REGISTER_PUT_TAKES_NO_BODY: &str = "A register `PUT` pastes captured lines and takes no `+` body rows. To write literal text, drop the `@name` and use `PUT …:` with body rows.";
const COLONLESS_PUT_TAKES_NO_BODY: &str = "`PUT` without `:` is clipboard-backed and takes no body rows. Add `:` after the locator to write literal content (`PUT >40:` then `+TEXT` rows).";
const COLONLESS_SPAN_PUT: &str = "Colonless `PUT` is clipboard-backed, and span targets need a named register (`PUT 5.=9 @name`); the anonymous register pastes only at gaps (`PUT >40`). To write literal content, add `:` and `+TEXT` body rows.";
const EMPTY_INSERT: &str = "`PUT <N:` / `PUT >N:` promises body rows and got none. Write `+TEXT` rows, or drop the `:` to paste a register (`PUT >N` = anonymous, `PUT >N @name` = named).";
const MAX_RANGE_SPAN: usize = 100_000;

/// Insertion point, in original line numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    Bof,
    Eof,
    Before(usize),
    After(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    /// `PUT N.=M:` + body.
    Replace { start: usize, end: usize, body: Vec<String> },
    /// `PUT <N:` / `PUT >N:` / `PUT >$:` + body.
    Insert { gap: Gap, body: Vec<String> },
    /// `CUT N.=M [@reg]`.
    Cut { start: usize, end: usize, register: Option<String> },
    /// `PUT <N [@reg]` (no body).
    PasteGap { gap: Gap, register: Option<String> },
    /// `PUT N.=M @reg`.
    PasteSpan { start: usize, end: usize, register: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub kind: OpKind,
    /// Patch line of the op header (1-based), for messages.
    pub line: usize,
}

impl Op {
    /// Lines the op anchors to (original numbering).
    pub fn anchors(&self) -> Vec<usize> {
        match &self.kind {
            OpKind::Replace { start, end, .. }
            | OpKind::Cut { start, end, .. }
            | OpKind::PasteSpan { start, end, .. } => (*start..=*end).collect(),
            OpKind::Insert { gap, .. } | OpKind::PasteGap { gap, .. } => match gap {
                Gap::Before(n) | Gap::After(n) => vec![*n],
                Gap::Bof | Gap::Eof => Vec::new(),
            },
        }
    }

    /// Lines removed/replaced by this op.
    pub fn span(&self) -> Option<(usize, usize)> {
        match &self.kind {
            OpKind::Replace { start, end, .. }
            | OpKind::Cut { start, end, .. }
            | OpKind::PasteSpan { start, end, .. } => Some((*start, *end)),
            _ => None,
        }
    }

    fn uses_clipboard(&self) -> bool {
        matches!(self.kind, OpKind::Cut { .. } | OpKind::PasteGap { .. } | OpKind::PasteSpan { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOp {
    Rem,
    Move(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderTag {
    Tag(String),
    /// `[path]` with no `#TAG`.
    Missing,
    /// `#AUTO`, `#????`, bare `#`, …
    Placeholder(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub path: String,
    pub tag: HeaderTag,
    pub line: usize,
    pub ops: Vec<Op>,
    pub file_op: Option<FileOp>,
    pub warnings: Vec<String>,
}

impl Section {
    pub fn tag(&self) -> Option<&str> {
        match &self.tag {
            HeaderTag::Tag(t) => Some(t),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Line classification helpers
// ---------------------------------------------------------------------------

static HEX4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Fa-f]{4}$").unwrap());
static GREP_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6} (\S.*)#([0-9A-Fa-f]{4})\s*$").unwrap());
static HL_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:(?:>>>|>>)\s*)?(?:[+*-]\s*)?(\d+)[:|]").unwrap());
static UNIFIED_HUNK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@@\s+[-+]?\d+(?:,\d+)?\s+[-+]?\d+(?:,\d+)?\s+@@").unwrap());
static RANGE_ELISION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[1-9]\d*\s*-\s*[1-9]\d*:.*(?:…|\.\.\.).*$").unwrap());

fn clean_header_path(raw: &str) -> String {
    let mut p = raw.trim();
    for noise in ["*** Update File:", "*** Add File:", "*** Delete File:", "Update File:"] {
        if let Some(rest) = p.strip_prefix(noise) {
            p = rest.trim();
        }
    }
    let p = p.trim_matches('`');
    let p = if p.len() >= 2
        && ((p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\'')))
    {
        &p[1..p.len() - 1]
    } else {
        p
    };
    p.trim().to_string()
}

/// Parse a section header. `allow_untagged` accepts `[path]` with no `#` (only before any section).
pub fn parse_header(line: &str, allow_untagged: bool) -> Option<(String, HeaderTag)> {
    let t = line.trim();
    if let Some(c) = GREP_HEADER_RE.captures(t) {
        let path = clean_header_path(&c[1]);
        if !path.is_empty() {
            return Some((path, HeaderTag::Tag(c[2].to_ascii_uppercase())));
        }
    }
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() || inner.contains('[') || inner.contains(']') {
        return None;
    }
    if let Some((path, hash)) = inner.rsplit_once('#') {
        let path = clean_header_path(path);
        if path.is_empty() {
            return None;
        }
        let hash = hash.trim();
        if HEX4.is_match(hash) {
            return Some((path, HeaderTag::Tag(hash.to_ascii_uppercase())));
        }
        if hash.len() <= 8 && !hash.contains(['/', ' ', '.']) {
            return Some((path, HeaderTag::Placeholder(hash.to_string())));
        }
        return None;
    }
    if allow_untagged {
        let path = clean_header_path(inner);
        if !path.is_empty() && !path.contains(char::is_whitespace) {
            return Some((path, HeaderTag::Missing));
        }
    }
    None
}

fn parse_number_prefix(raw: &str) -> Option<(usize, usize)> {
    let bytes = raw.as_bytes();
    if !matches!(bytes.first(), Some(b'1'..=b'9')) {
        return None;
    }
    let end = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    Some((raw[..end].parse().ok()?, end))
}

/// Parse `N`, `N.=M`, `N-M`, `N..M`, `N … M`, `N - M`. Returns (start, end, bytes used).
fn parse_range(raw: &str) -> Option<(usize, usize, usize)> {
    let lead = raw.len() - raw.trim_start().len();
    let (start, used) = parse_number_prefix(&raw[lead..])?;
    let mut cur = lead + used;
    let after_start = cur;
    while let Some(ch) = raw[cur..].chars().next() {
        if ch.is_whitespace() || matches!(ch, '-' | '.' | '=' | '…') {
            cur += ch.len_utf8();
        } else {
            break;
        }
    }
    if let Some((end, n)) = parse_number_prefix(&raw[cur..]) {
        return Some((start, end, cur + n));
    }
    Some((start, start, after_start))
}

enum Locator {
    Range(usize, usize),
    Gap(Gap),
}

struct OpHeader {
    put: bool,
    loc: Locator,
    register: Option<String>,
    colon: bool,
}

enum HeaderParse {
    Op(OpHeader),
    Rem,
    Move(String),
    Block,
    Malformed,
    NotOp,
}

fn keyword_tail<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(kw)?;
    (rest.is_empty() || rest.starts_with(':') || rest.starts_with(char::is_whitespace)).then_some(rest)
}

fn parse_reg_colon(rest: &str) -> Option<(Option<String>, bool)> {
    let mut rest = rest.trim_start();
    let mut register = None;
    if let Some(r) = rest.strip_prefix('@') {
        let n = r.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')).count();
        if n == 0 || n > 64 {
            return None;
        }
        register = Some(r[..n].to_string());
        rest = r[n..].trim_start();
    }
    let colon = rest.starts_with(':');
    if colon {
        rest = rest[1..].trim_start();
    }
    rest.is_empty().then_some((register, colon))
}

fn classify_op(line: &str) -> HeaderParse {
    let t = line.trim();
    if let Some(rest) = keyword_tail(t, "REM") {
        return if rest.trim().is_empty() { HeaderParse::Rem } else { HeaderParse::Malformed };
    }
    if let Some(rest) = keyword_tail(t, "MV") {
        let d = rest.trim();
        let d = if d.len() >= 2
            && ((d.starts_with('"') && d.ends_with('"')) || (d.starts_with('\'') && d.ends_with('\'')))
        {
            &d[1..d.len() - 1]
        } else {
            d
        };
        return if d.is_empty() { HeaderParse::Malformed } else { HeaderParse::Move(d.to_string()) };
    }
    let (put, rest) = if let Some(r) = keyword_tail(t, "PUT") {
        (true, r.trim_start())
    } else if let Some(r) = keyword_tail(t, "CUT") {
        (false, r.trim_start())
    } else {
        return HeaderParse::NotOp;
    };
    let (loc, tail) = if put && (rest.starts_with('<') || rest.starts_with('>')) {
        let before = rest.starts_with('<');
        let after = rest[1..].trim_start();
        if !before && after.starts_with('$') {
            (Locator::Gap(Gap::Eof), &after[1..])
        } else {
            let Some((n, used)) = parse_number_prefix(after) else { return HeaderParse::Malformed };
            let tail = &after[used..];
            if tail.starts_with('*') {
                return HeaderParse::Block;
            }
            let gap = if before {
                if n == 1 { Gap::Bof } else { Gap::Before(n) }
            } else {
                Gap::After(n)
            };
            (Locator::Gap(gap), tail)
        }
    } else {
        let Some((s, e, used)) = parse_range(rest) else { return HeaderParse::Malformed };
        let tail = &rest[used..];
        if tail.trim_start().starts_with('*') {
            return HeaderParse::Block;
        }
        (Locator::Range(s, e), tail)
    };
    match parse_reg_colon(tail) {
        Some((register, colon)) => HeaderParse::Op(OpHeader { put, loc, register, colon }),
        None => HeaderParse::Malformed,
    }
}

fn is_read_metadata(line: &str) -> bool {
    let t = line.trim();
    if t == "…" || t == "..." || RANGE_ELISION_RE.is_match(t) {
        return true;
    }
    let Some(body) = t.strip_prefix('[').and_then(|b| b.strip_suffix(']')) else { return false };
    (body.contains(" more lines") && body.contains("continue"))
        || body.starts_with("Showing ")
        || body.contains("ln elided")
        || body.starts_with("output truncated")
}

fn contamination(text: &str) -> Option<String> {
    let t = text.trim_start();
    if ["*** Update File:", "*** Add File:", "*** Delete File:", "*** Move to:"]
        .iter()
        .any(|p| t.starts_with(p))
    {
        return Some(format!(
            "apply_patch sentinel {:?} is not valid in hashline. File sections start with `[path#TAG]` (no `Update File:` keyword). Use `PUT N.=M:`, `CUT N.=M`, or `PUT <N:`/`PUT >N:` ops.",
            crate::util::truncate_chars(t, 48).0
        ));
    }
    if t.starts_with("@@") {
        if UNIFIED_HUNK_RE.is_match(t) {
            return Some("unified-diff hunk header (`@@ -N,M +N,M @@`) is not valid in hashline. Use `PUT N.=M:`, `CUT N.=M`, or `PUT <N:`/`PUT >N:` ops.".into());
        }
        return Some(format!(
            "`@@`-bracketed hunk header {:?} is not valid in hashline. Drop the `@@ ... @@` brackets and write a header such as `PUT N.=M:`.",
            crate::util::truncate_chars(t, 48).0
        ));
    }
    let pieces: Vec<&str> = t.trim_end_matches(':').split_whitespace().collect();
    if !pieces.is_empty() && pieces.len() <= 2 && pieces.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        let n = pieces[0];
        return Some(format!(
            "hunk headers need a verb and both endpoints. Use `PUT {n}.={n}:` to replace, or `CUT {n}.={n}` to delete."
        ));
    }
    None
}

fn parse_bare_range(text: &str) -> Option<(usize, usize)> {
    let before = text.trim().strip_suffix(':')?.trim();
    let mut nums = before
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '.' | '=' | '…'))
        .filter(|p| !p.is_empty());
    let s: usize = nums.next()?.parse().ok()?;
    let e: usize = nums.next()?.parse().ok()?;
    (nums.next().is_none() && s > 0 && e > 0).then_some((s, e))
}

fn parse_snapshot_row(text: &str) -> Option<(usize, String)> {
    let t = text.trim_start();
    let split = t.find([':', '|'])?;
    let num = &t[..split];
    if num.is_empty() || num.starts_with('0') || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((num.parse().ok()?, t[split + 1..].to_string()))
}

fn markdown_bullet(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("- ") && t[2..].chars().next().is_some_and(|c| !c.is_whitespace())
}

fn literal_value(text: &str) -> bool {
    let v = text.trim().trim_end_matches(',').trim();
    (v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))))
        || v.parse::<f64>().is_ok()
}

/// Strip one copied `N:` / `N|` display prefix.
pub fn strip_display_prefix(line: &str) -> String {
    HL_PREFIX_RE.replace(line, "").into_owned()
}

pub fn has_display_prefix(line: &str) -> bool {
    HL_PREFIX_RE.is_match(line)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Row {
    text: String,
    line: usize,
    bare: bool,
    minus: bool,
}

struct Pending {
    hdr: OpHeader,
    line: usize,
    rows: Vec<Row>,
    deferred_blanks: Vec<Row>,
}

#[derive(Default)]
struct SectionBuilder {
    path: String,
    tag: Option<HeaderTag>,
    line: usize,
    ops: Vec<Op>,
    file_op: Option<FileOp>,
    warnings: Vec<String>,
    pending: Option<Pending>,
    snapshot_rows: Vec<usize>,
    saw_op: bool,
}

fn fail<T>(line: usize, msg: impl AsRef<str>) -> Result<T, String> {
    Err(format!("line {line}: {}", msg.as_ref()))
}

impl SectionBuilder {
    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
    }

    fn bodyless(hdr: &OpHeader) -> Option<&'static str> {
        if !hdr.put {
            Some(CUT_TAKES_NO_BODY)
        } else if hdr.register.is_some() {
            Some(REGISTER_PUT_TAKES_NO_BODY)
        } else if !hdr.colon {
            Some(COLONLESS_PUT_TAKES_NO_BODY)
        } else {
            None
        }
    }

    fn start_op(&mut self, hdr: OpHeader, line: usize) -> Result<(), String> {
        self.flush()?;
        if matches!(self.file_op, Some(FileOp::Rem)) {
            return fail(line, REM_TAKES_NO_BODY);
        }
        if let Locator::Range(s, e) = hdr.loc {
            let op = if hdr.put { "PUT" } else { "CUT" };
            if e < s {
                return fail(
                    line,
                    format!(
                        "Invalid absolute range: start {s}, end {e}. The value after `.=` is an absolute source line, not a line count. For one line use `{op} {s}.={s}`; for {e} lines starting at {s}, use `{op} {s}.={}`.",
                        s + e - 1
                    ),
                );
            }
            if e - s + 1 > MAX_RANGE_SPAN {
                return fail(line, format!("range spans {} lines; the maximum is {MAX_RANGE_SPAN}. Split it into smaller hunks.", e - s + 1));
            }
        }
        if !hdr.put && hdr.colon {
            self.warn(CUT_COLON_WARNING);
        }
        if hdr.put && hdr.colon && hdr.register.is_some() {
            return fail(line, COLON_ON_REGISTER_PUT);
        }
        self.saw_op = true;
        self.pending = Some(Pending { hdr, line, rows: Vec::new(), deferred_blanks: Vec::new() });
        Ok(())
    }

    fn body_row(&mut self, text: &str, line: usize, bare: bool) -> Result<(), String> {
        let Some(p) = self.pending.as_mut() else {
            if self.file_op.is_some() {
                return fail(line, MOVE_TAKES_NO_BODY);
            }
            return fail(line, format!("payload line has no preceding hunk header. Use `PUT N.=M:`, `CUT N.=M`, or `PUT <N:`/`PUT >N:` above the body. Got {:?}.", if bare { text.to_string() } else { format!("+{text}") }));
        };
        if let Some(msg) = Self::bodyless(&p.hdr) {
            return fail(line, msg);
        }
        let minus = bare && text.trim_start().starts_with('-');
        p.rows.append(&mut p.deferred_blanks);
        p.rows.push(Row { text: text.to_string(), line, bare, minus });
        if bare && !minus {
            self.warn(BARE_BODY_WARNING);
        }
        Ok(())
    }

    fn blank(&mut self, line: usize) {
        if let Some(p) = self.pending.as_mut() {
            if Self::bodyless(&p.hdr).is_none() && !p.rows.is_empty() {
                p.deferred_blanks.push(Row { text: String::new(), line, bare: true, minus: false });
            }
        }
    }

    fn raw(&mut self, text: &str, line: usize) -> Result<(), String> {
        if self.pending.is_none() && is_read_metadata(text) {
            self.warn(METADATA_IGNORED_WARNING);
            return Ok(());
        }
        if let Some(msg) = contamination(text) {
            return fail(line, msg);
        }
        if self.file_op.is_some() && self.pending.is_none() {
            return fail(line, MOVE_TAKES_NO_BODY);
        }
        if self.pending.is_some() {
            return self.body_row(text, line, true);
        }
        if text.trim_start().starts_with('#') {
            return Ok(()); // comment row outside a hunk
        }
        if let Some((s, e)) = parse_bare_range(text) {
            self.warn(BARE_RANGE_WARNING);
            return self.start_op(
                OpHeader { put: true, loc: Locator::Range(s, e), register: None, colon: true },
                line,
            );
        }
        if let Some((n, value)) = parse_snapshot_row(text) {
            if self.snapshot_rows.contains(&n) {
                return fail(line, format!("two or more pasted `{n}:TEXT` read-output rows name line {n}. Write the hunk explicitly: one `PUT {n}.=M:` header covering exactly the lines that change, followed by `+TEXT` body rows holding their complete final content."));
            }
            self.snapshot_rows.push(n);
            self.saw_op = true;
            self.ops.push(Op { kind: OpKind::Replace { start: n, end: n, body: vec![value] }, line });
            self.warn(SNAPSHOT_ROWS_WARNING);
            return Ok(());
        }
        self.body_row(text, line, true)
    }

    fn resolve_minus(&mut self, rows: &mut Vec<Row>) -> Result<(), String> {
        let minus: Vec<&Row> = rows.iter().filter(|r| r.minus).collect();
        if minus.is_empty() {
            return Ok(());
        }
        let all_bullets = minus.iter().all(|r| markdown_bullet(&r.text));
        let explicit: Vec<&Row> = rows.iter().filter(|r| !r.bare).collect();
        if all_bullets && (explicit.is_empty() || explicit.iter().any(|r| markdown_bullet(&r.text))) {
            self.warn(MINUS_BULLET_WARNING);
            return Ok(());
        }
        if !explicit.is_empty() && !all_bullets {
            rows.retain(|r| !r.minus);
            self.warn(DIFF_OLD_ROWS_WARNING);
            return Ok(());
        }
        fail(minus[0].line, MINUS_ROW_REJECTED)
    }

    fn strip_uniform_prefixes(&mut self, rows: &mut [Row]) {
        let bare: Vec<&Row> = rows.iter().filter(|r| r.bare && !r.text.trim().is_empty()).collect();
        if bare.is_empty() {
            return;
        }
        let mut all_literal = true;
        for r in &bare {
            if !has_display_prefix(&r.text) {
                return;
            }
            all_literal &= literal_value(&strip_display_prefix(&r.text));
        }
        if all_literal {
            return;
        }
        for r in rows.iter_mut().filter(|r| r.bare && !r.text.trim().is_empty()) {
            r.text = strip_display_prefix(&r.text);
        }
        self.warn(PREFIX_STRIPPED_WARNING);
    }

    fn flush(&mut self) -> Result<(), String> {
        let Some(p) = self.pending.take() else { return Ok(()) };
        let mut rows = p.rows;
        self.resolve_minus(&mut rows)?;
        self.strip_uniform_prefixes(&mut rows);
        let body: Vec<String> = rows.into_iter().map(|r| r.text).collect();
        let line = p.line;
        let hdr = p.hdr;
        let kind = match (hdr.put, hdr.loc) {
            (false, Locator::Range(start, end)) => OpKind::Cut { start, end, register: hdr.register },
            (false, Locator::Gap(_)) => return fail(line, "`CUT` takes a line range (`CUT N.=M`)."),
            (true, Locator::Range(start, end)) => {
                if let Some(register) = hdr.register {
                    OpKind::PasteSpan { start, end, register }
                } else if body.is_empty() {
                    if !hdr.colon {
                        return fail(line, COLONLESS_SPAN_PUT);
                    }
                    self.warn(EMPTY_PUT_WARNING);
                    OpKind::Cut { start, end, register: None }
                } else {
                    OpKind::Replace { start, end, body }
                }
            }
            (true, Locator::Gap(gap)) => {
                if hdr.register.is_some() || (!hdr.colon && body.is_empty()) {
                    OpKind::PasteGap { gap, register: hdr.register }
                } else if body.is_empty() {
                    return fail(line, EMPTY_INSERT);
                } else {
                    OpKind::Insert { gap, body }
                }
            }
        };
        self.ops.push(Op { kind, line });
        Ok(())
    }

    fn set_file_op(&mut self, op: FileOp, line: usize) -> Result<(), String> {
        self.flush()?;
        if self.file_op.is_some() {
            return fail(line, "only one file-level op (`REM` or `MV`) per section. Merge them under one header.");
        }
        if matches!(op, FileOp::Rem) && !self.ops.is_empty() {
            return fail(line, REM_TAKES_NO_BODY);
        }
        self.saw_op = true;
        self.file_op = Some(op);
        Ok(())
    }

    fn finish(mut self) -> Result<Section, String> {
        self.flush()?;
        if matches!(self.file_op, Some(FileOp::Rem)) && !self.ops.is_empty() {
            return fail(self.line, REM_TAKES_NO_BODY);
        }
        normalize_overlaps(&mut self.ops, &mut self.warnings)?;
        Ok(Section {
            path: self.path,
            tag: self.tag.unwrap_or(HeaderTag::Missing),
            line: self.line,
            ops: self.ops,
            file_op: self.file_op,
            warnings: self.warnings,
        })
    }
}

/// Reject overlapping ranges; exact duplicates keep the last (with a warning).
pub fn normalize_overlaps(ops: &mut Vec<Op>, warnings: &mut Vec<String>) -> Result<(), String> {
    let mut keep = vec![true; ops.len()];
    for i in 0..ops.len() {
        let Some((s, e)) = ops[i].span() else { continue };
        for j in 0..i {
            if !keep[j] {
                continue;
            }
            let Some((ps, pe)) = ops[j].span() else { continue };
            if s > pe || ps > e {
                continue;
            }
            if (ps, pe) == (s, e) && !ops[i].uses_clipboard() && !ops[j].uses_clipboard() {
                keep[j] = false;
                if !warnings.iter().any(|w| w == DUP_RANGE_WARNING) {
                    warnings.push(DUP_RANGE_WARNING.to_string());
                }
                continue;
            }
            let first = s.max(ps);
            return fail(
                ops[i].line,
                format!(
                    "anchor line {first} is already targeted by another hunk on line {}. Issue ONE hunk per range; payload is only the final desired content, never a before/after pair.",
                    ops[j].line
                ),
            );
        }
    }
    let mut it = keep.into_iter();
    ops.retain(|_| it.next().unwrap_or(true));
    Ok(())
}

/// Strip a surrounding markdown code fence, if the whole input is fenced.
fn strip_fence(input: &str) -> &str {
    let t = input.trim();
    if t.starts_with("```") && t.ends_with("```") && t.len() > 6 {
        if let Some(nl) = t.find('\n') {
            let inner = &t[nl + 1..t.len() - 3];
            return inner;
        }
    }
    input
}

/// Parse a full patch into sections (same-path sections are merged).
pub fn parse_patch(input: &str) -> Result<Vec<Section>, String> {
    let input = strip_fence(input);
    let mut sections: Vec<Section> = Vec::new();
    let mut cur: Option<SectionBuilder> = None;
    for (idx, raw_line) in input.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed_end = line.trim_end();
        match trimmed_end {
            "*** Begin Patch" => continue,
            "*** End Patch" | "*** Abort" => break,
            _ => {}
        }
        if let Some(text) = line.strip_prefix('+') {
            match cur.as_mut() {
                Some(b) => b.body_row(text, line_no, false)?,
                None => return fail(line_no, "body row before any `[PATH#TAG]` header. Start each file section with the header copied from `read`."),
            }
            continue;
        }
        if line.trim().is_empty() {
            if let Some(b) = cur.as_mut() {
                b.blank(line_no);
            }
            continue;
        }
        if let Some((path, tag)) = parse_header(line, cur.is_none()) {
            if let Some(b) = cur.take() {
                sections.push(b.finish()?);
            }
            cur = Some(SectionBuilder { path, tag: Some(tag), line: line_no, ..Default::default() });
            continue;
        }
        let Some(b) = cur.as_mut() else {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some(msg) = contamination(line) {
                return fail(line_no, msg);
            }
            return fail(line_no, format!("expected a `[PATH#TAG]` file header (copy it from `read`/`grep` output); got {:?}.", crate::util::truncate_chars(line, 80).0));
        };
        match classify_op(line) {
            HeaderParse::Op(hdr) => b.start_op(hdr, line_no)?,
            HeaderParse::Rem => b.set_file_op(FileOp::Rem, line_no)?,
            HeaderParse::Move(dest) => b.set_file_op(FileOp::Move(dest), line_no)?,
            HeaderParse::Block => return fail(line_no, BLOCK_RESOLVER_UNAVAILABLE),
            HeaderParse::Malformed => {
                return fail(line_no, format!("malformed op header {:?}. Forms: `PUT N.=M:`, `PUT <N:`, `PUT >N:`, `PUT >$:`, `CUT N.=M [@name]`, `PUT >N [@name]`, `PUT N.=M @name`, `REM`, `MV DEST`.", line.trim()))
            }
            HeaderParse::NotOp => b.raw(line, line_no)?,
        }
    }
    if let Some(b) = cur.take() {
        sections.push(b.finish()?);
    }
    if sections.is_empty() {
        return Err("empty patch: expected `[PATH#TAG]` followed by ops such as `PUT N.=M:` with `+TEXT` rows.".into());
    }
    // Merge same-path sections into the first occurrence.
    let mut merged: Vec<Section> = Vec::new();
    for s in sections {
        if let Some(prev) = merged.iter_mut().find(|m| m.path == s.path) {
            if prev.tag != s.tag {
                return fail(s.line, format!("section for {} repeats with a different tag than the section on line {}. Use one `[PATH#TAG]` header per file.", s.path, prev.line));
            }
            if prev.file_op.is_some() || (s.file_op.is_some() && prev.file_op.is_some()) {
                return fail(s.line, format!("{} already has a `REM`/`MV` in the section on line {}; put all ops for a file under one header.", s.path, prev.line));
            }
            prev.ops.extend(s.ops);
            prev.file_op = s.file_op;
            for w in s.warnings {
                if !prev.warnings.contains(&w) {
                    prev.warnings.push(w);
                }
            }
            if matches!(prev.file_op, Some(FileOp::Rem)) && !prev.ops.is_empty() {
                return fail(s.line, REM_TAKES_NO_BODY);
            }
            let mut w = std::mem::take(&mut prev.warnings);
            normalize_overlaps(&mut prev.ops, &mut w)?;
            prev.warnings = w;
        } else {
            merged.push(s);
        }
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(input: &str) -> Section {
        let mut s = parse_patch(input).unwrap();
        assert_eq!(s.len(), 1);
        s.remove(0)
    }

    #[test]
    fn parses_basic_ops() {
        let s = one("[a.rs#0A1B]\nPUT 2.=3:\n+x\n+\n+  y\nPUT <1:\n+head\nPUT >$:\n+tail\nPUT >5:\n+after\nCUT 7.=8 @m\nPUT <10 @m\n");
        assert_eq!(s.path, "a.rs");
        assert_eq!(s.tag(), Some("0A1B"));
        assert_eq!(s.ops.len(), 6);
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 2, end: 3, body: vec!["x".into(), "".into(), "  y".into()] });
        assert_eq!(s.ops[1].kind, OpKind::Insert { gap: Gap::Bof, body: vec!["head".into()] });
        assert_eq!(s.ops[2].kind, OpKind::Insert { gap: Gap::Eof, body: vec!["tail".into()] });
        assert_eq!(s.ops[3].kind, OpKind::Insert { gap: Gap::After(5), body: vec!["after".into()] });
        assert_eq!(s.ops[4].kind, OpKind::Cut { start: 7, end: 8, register: Some("m".into()) });
        assert_eq!(s.ops[5].kind, OpKind::PasteGap { gap: Gap::Before(10), register: Some("m".into()) });
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn envelope_and_separators() {
        let s = one("*** Begin Patch\n[a.rs#abcd]\nPUT 2-3:\n+x\nPUT 5..6:\n+y\nCUT 8 .. 9\nPUT 11:\n+z\n*** End Patch\nignored garbage");
        assert_eq!(s.tag(), Some("ABCD"));
        assert_eq!(s.ops[0].span(), Some((2, 3)));
        assert_eq!(s.ops[1].span(), Some((5, 6)));
        assert_eq!(s.ops[2].span(), Some((8, 9)));
        assert_eq!(s.ops[3].span(), Some((11, 11)));
    }

    #[test]
    fn leniencies() {
        // bare rows auto-prefixed
        let s = one("[a#AAAA]\nPUT 1.=1:\n  let x = 1;\n");
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 1, end: 1, body: vec!["  let x = 1;".into()] });
        assert!(s.warnings[0].contains("Auto-prefixed"));
        // copied N: prefixes stripped
        let s = one("[a#AAAA]\nPUT 1.=2:\n1:foo\n2:  bar\n");
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 1, end: 2, body: vec!["foo".into(), "  bar".into()] });
        // empty PUT → CUT
        let s = one("[a#AAAA]\nPUT 3.=4:\nPUT >9:\n+x\n");
        assert_eq!(s.ops[0].kind, OpKind::Cut { start: 3, end: 4, register: None });
        assert!(s.warnings.iter().any(|w| w.contains("empty `PUT`")));
        // bare range header + snapshot rows
        let s = one("[a#AAAA]\n4.=5:\n+q\n");
        assert_eq!(s.ops[0].span(), Some((4, 5)));
        let s = one("[a#AAAA]\n7:new seven\n");
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 7, end: 7, body: vec!["new seven".into()] });
        // diff-contamination: explicit + rows and - rows → minus dropped
        let s = one("[a#AAAA]\nPUT 1.=1:\n-old\n+new\n");
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 1, end: 1, body: vec!["new".into()] });
        // bullets
        let s = one("[a.md#AAAA]\nPUT >1:\n- item one\n- item two\n");
        assert_eq!(s.ops[0].kind, OpKind::Insert { gap: Gap::After(1), body: vec!["- item one".into(), "- item two".into()] });
        // trailing blank lines in body dropped, inner kept
        let s = one("[a#AAAA]\nPUT 1.=1:\n+a\n\n+b\n\n\n");
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 1, end: 1, body: vec!["a".into(), "".into(), "b".into()] });
        // grep-style header
        let s = one("## src/x.rs#1f2a\nCUT 1.=1\n");
        assert_eq!((s.path.as_str(), s.tag()), ("src/x.rs", Some("1F2A")));
    }

    #[test]
    fn errors() {
        let e = parse_patch("[a#AAAA]\nPUT 5.=3:\n+x\n").unwrap_err();
        assert!(e.contains("Invalid absolute range"), "{e}");
        let e = parse_patch("[a#AAAA]\nPUT 3.=5:\n+x\nPUT 4.=6:\n+y\n").unwrap_err();
        assert!(e.contains("already targeted"), "{e}");
        let e = parse_patch("[a#AAAA]\nPUT 3*:\n+x\n").unwrap_err();
        assert!(e.contains("Block locators"), "{e}");
        let e = parse_patch("[a#AAAA]\nCUT 3.=4\n+x\n").unwrap_err();
        assert!(e.contains("takes no"), "{e}");
        let e = parse_patch("[a#AAAA]\nPUT >3:\n").unwrap_err();
        assert!(e.contains("promises body rows"), "{e}");
        let e = parse_patch("[a#AAAA]\n@@ -1,2 +1,2 @@\n").unwrap_err();
        assert!(e.contains("unified-diff"), "{e}");
        let e = parse_patch("[a#AAAA]\nPUT 1.=1:\n-old\n").unwrap_err();
        assert!(e.contains("`-` rows are not valid"), "{e}");
        let e = parse_patch("[a#AAAA]\nREM\nPUT 1.=1:\n+x\n").unwrap_err();
        assert!(e.contains("REM"), "{e}");
        let e = parse_patch("[a#AAAA]\nPUT >2 @r:\n").unwrap_err();
        assert!(e.contains("never takes `:`"), "{e}");
    }

    #[test]
    fn duplicate_range_keeps_last_and_headers() {
        let s = one("[a#AAAA]\nPUT 2.=2:\n+x\nPUT 2.=2:\n+y\n");
        assert_eq!(s.ops.len(), 1);
        assert_eq!(s.ops[0].kind, OpKind::Replace { start: 2, end: 2, body: vec!["y".into()] });
        let s = one("[a.rs#AUTO]\nPUT 1.=1:\n+x\n");
        assert_eq!(s.tag, HeaderTag::Placeholder("AUTO".into()));
        let s = one("[a.rs]\nPUT 1.=1:\n+x\n");
        assert_eq!(s.tag, HeaderTag::Missing);
        let secs = parse_patch("[a#AAAA]\nMV \"b c.rs\"\n[d#BBBB]\nREM\n").unwrap();
        assert_eq!(secs[0].file_op, Some(FileOp::Move("b c.rs".into())));
        assert_eq!(secs[1].file_op, Some(FileOp::Rem));
    }
}
