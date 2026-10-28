//! Tiny, dependency-free syntax highlighter for fenced code blocks.
//! Handles comments, strings, numbers, keywords, types and function calls for
//! common languages. Good enough for terminal display; never fails.

use crate::theme;
use ratatui::style::Style;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    CLike,
    Rust,
    Python,
    Shell,
    Data,
    Sql,
    Diff,
    Plain,
}

const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "Some", "None", "Ok", "Err",
];
const C_KW: &[&str] = &[
    "auto", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "enum", "export",
    "extends", "false", "finally", "for", "func", "function", "go", "if", "import", "in", "instanceof", "interface",
    "let", "new", "null", "nil", "package", "private", "protected", "public", "return", "static", "struct", "switch",
    "this", "throw", "true", "try", "type", "typeof", "var", "void", "while", "yield", "async", "await", "from", "of",
    "defer", "chan", "map", "range", "select", "undefined", "int", "char", "float", "double", "long", "unsigned",
    "bool", "string", "namespace", "template", "typename", "virtual", "override", "implements", "abstract", "final",
];
const PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or",
    "pass", "raise", "return", "True", "try", "while", "with", "yield", "self", "match", "case",
];
const SH_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in", "function", "return",
    "export", "local", "echo", "cd", "exit", "set", "unset", "source", "sudo",
];
const SQL_KW: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop",
    "alter", "join", "left", "right", "inner", "outer", "on", "and", "or", "not", "null", "group", "by", "order",
    "limit", "as", "primary", "key", "index", "having", "distinct", "union", "all",
];
const DATA_KW: &[&str] = &["true", "false", "null", "yes", "no", "on", "off"];

fn family(lang: &str) -> Family {
    match lang.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Family::Rust,
        "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "go"
        | "golang" | "java" | "kotlin" | "kt" | "swift" | "cs" | "csharp" | "scala" | "dart" | "php" | "zig" => {
            Family::CLike
        }
        "py" | "python" | "rb" | "ruby" => Family::Python,
        "sh" | "bash" | "zsh" | "shell" | "fish" | "console" => Family::Shell,
        "json" | "jsonc" | "json5" | "toml" | "yaml" | "yml" | "ini" => Family::Data,
        "sql" | "lua" => Family::Sql,
        "diff" | "patch" => Family::Diff,
        _ => Family::Plain,
    }
}

/// Highlighter with cross-line state (block comments).
pub struct Highlighter {
    fam: Family,
    in_block_comment: bool,
}

impl Highlighter {
    pub fn new(lang: &str) -> Self {
        Highlighter { fam: family(lang), in_block_comment: false }
    }

    /// Highlight one line (no newline) into styled segments. `base` is the code text style.
    pub fn line(&mut self, line: &str, base: Style) -> Vec<(String, Style)> {
        let st = |c| base.fg(c);
        match self.fam {
            Family::Plain => return vec![(line.to_string(), base)],
            Family::Diff => {
                let c = if line.starts_with("+++") || line.starts_with("---") {
                    theme::MUTED
                } else if line.starts_with('+') {
                    theme::SUCCESS
                } else if line.starts_with('-') {
                    theme::ERROR
                } else if line.starts_with("@@") {
                    theme::INFO
                } else {
                    theme::TEXT
                };
                return vec![(line.to_string(), st(c))];
            }
            _ => {}
        }
        let kws: &[&str] = match self.fam {
            Family::Rust => RUST_KW,
            Family::CLike => C_KW,
            Family::Python => PY_KW,
            Family::Shell => SH_KW,
            Family::Sql => SQL_KW,
            Family::Data => DATA_KW,
            _ => &[],
        };
        let hash_comment = matches!(self.fam, Family::Python | Family::Shell | Family::Data);
        let slash_comment = matches!(self.fam, Family::Rust | Family::CLike);
        let dash_comment = self.fam == Family::Sql;

        let chars: Vec<char> = line.chars().collect();
        let mut out: Vec<(String, Style)> = Vec::new();
        let push = |out: &mut Vec<(String, Style)>, s: String, style: Style| {
            if s.is_empty() {
                return;
            }
            if let Some(last) = out.last_mut() {
                if last.1 == style {
                    last.0.push_str(&s);
                    return;
                }
            }
            out.push((s, style));
        };
        let mut i = 0;
        while i < chars.len() {
            if self.in_block_comment {
                let start = i;
                while i < chars.len() {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        i += 2;
                        self.in_block_comment = false;
                        break;
                    }
                    i += 1;
                }
                push(&mut out, chars[start..i].iter().collect(), st(theme::SYN_COMMENT).italic());
                continue;
            }
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            // Comments
            if (slash_comment && c == '/' && next == Some('/'))
                || (hash_comment && c == '#')
                || (dash_comment && c == '-' && next == Some('-'))
            {
                push(&mut out, chars[i..].iter().collect(), st(theme::SYN_COMMENT).italic());
                break;
            }
            if slash_comment && c == '/' && next == Some('*') {
                self.in_block_comment = true;
                push(&mut out, "/*".into(), st(theme::SYN_COMMENT).italic());
                i += 2;
                continue;
            }
            // Strings
            let is_quote = c == '"' || c == '`' || (c == '\'' && self.fam != Family::Rust);
            let rust_char = self.fam == Family::Rust
                && c == '\''
                && (chars.get(i + 2) == Some(&'\'') || (next == Some('\\') && chars.get(i + 3) == Some(&'\'')));
            if is_quote || rust_char {
                let q = c;
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == q {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let i2 = i.min(chars.len());
                push(&mut out, chars[start..i2].iter().collect(), st(theme::SYN_STRING));
                i = i2;
                continue;
            }
            // Numbers
            if c.is_ascii_digit() && (i == 0 || !is_ident(chars[i - 1])) {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                    i += 1;
                }
                push(&mut out, chars[start..i].iter().collect(), st(theme::SYN_NUMBER));
                continue;
            }
            // Identifiers
            if is_ident_start(c) || (self.fam == Family::Shell && c == '$') {
                let start = i;
                i += 1;
                while i < chars.len() && (is_ident(chars[i]) || (self.fam == Family::Shell && chars[i] == '-')) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let style = if c == '$' {
                    st(theme::SYN_TYPE)
                } else if kws.iter().any(|k| {
                    if self.fam == Family::Sql {
                        k.eq_ignore_ascii_case(&word)
                    } else {
                        *k == word
                    }
                }) {
                    st(theme::SYN_KEYWORD)
                } else if chars.get(i) == Some(&'(')
                    || (self.fam == Family::Rust && chars.get(i) == Some(&'!'))
                    || (self.fam == Family::Data && line.trim_start().starts_with(word.as_str()) && {
                        let rest: String = chars[i..].iter().collect();
                        let r = rest.trim_start();
                        r.starts_with(':') || r.starts_with('=')
                    })
                {
                    st(theme::SYN_FUNCTION)
                } else if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && self.fam != Family::Shell {
                    st(theme::SYN_TYPE)
                } else {
                    base
                };
                push(&mut out, word, style);
                continue;
            }
            let style = if "{}[]()<>;,.:=+-*/&|!?%^~@".contains(c) { st(theme::SYN_PUNCT) } else { base };
            push(&mut out, c.to_string(), style);
            i += 1;
        }
        if out.is_empty() {
            out.push((String::new(), base));
        }
        out
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust() {
        let mut h = Highlighter::new("rust");
        let segs = h.line("fn main() { let x = \"hi\"; // c", Style::default());
        let joined: String = segs.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(joined, "fn main() { let x = \"hi\"; // c");
        assert_eq!(segs[0].1.fg, Some(theme::SYN_KEYWORD));
        assert!(segs.iter().any(|s| s.0 == "\"hi\"" && s.1.fg == Some(theme::SYN_STRING)));
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut h = Highlighter::new("c");
        h.line("/* start", Style::default());
        let segs = h.line("still */ int", Style::default());
        assert_eq!(segs[0].1.fg, Some(theme::SYN_COMMENT));
        assert!(segs.iter().any(|s| s.0 == "int" && s.1.fg == Some(theme::SYN_KEYWORD)));
    }
}
