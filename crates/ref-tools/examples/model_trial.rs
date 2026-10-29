//! Dev-only harness: run a real model against the built-in tools on a small multi-file editing
//! task and measure how often its edit calls fail.
//!
//! cargo run -p ref-tools --example model_trial -- --model zai/glm-5.3 [--runs 3] [--steps 25]
//!     [--log /tmp/opencode/trials] [--keep]
//!
//! Loads the user's real config (API keys are never printed). Writes a full transcript per run to
//! `<log>/<model>-<n>.log` and prints one summary line per run.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::StreamExt;
use ref_llm::{FinishReason, Request, StreamEvent};
use ref_proto::{Message, MessageOrigin, Part, ReasoningPart, Role, TextPart, ToolPart, ToolState};
use serde_json::{json, Value};

const FIXTURES: &[(&str, &str)] = &[
    (
        "src/inventory.rs",
        include_str!("trial_fixtures/inventory.rs"),
    ),
    ("src/cli.rs", include_str!("trial_fixtures/cli.rs")),
    ("stats.py", include_str!("trial_fixtures/stats.py")),
    ("README.md", include_str!("trial_fixtures/README.md")),
    ("settings.ini", include_str!("trial_fixtures/settings.ini")),
];

const HANDLERS: &str = include_str!("trial_fixtures/handlers.py");

/// Task 2: many scattered edits in one long file, bash restricted to read-only commands, and an
/// external modification of the file after the model's first read (stale-tag path).
const TASK2: &str = "In handlers.py (a long generated module) make these changes:
1. In `Handler07`, `Handler23`, `Handler45` and `Handler59`, set `retries = 10`.
2. In `Handler12.run`, change the multiplier in the returned dict to 100 (keep everything else).
3. Delete the whole `Handler31` class (and its trailing blank lines), and remove `Handler31` from the list in `register_all()`.
4. Add a method `def reset(self) -> None:` with body `self.retries = 0` to `Handler50`, right after its `describe` method.
5. Add a function `def names() -> list[str]:` returning `sorted(REGISTRY)` at the very end of the file.
Use the edit tool for the changes (bash is read-only in this environment). Do not change anything else.";

const TASK: &str = "Please make these changes in this repository (a Rust library in src/ plus a Python module):
1. Rename the Rust function `calc_total` to `order_total` everywhere (definition, all call sites, docs in README.md).
2. Add `pub fn total_quantity(items: &[Item]) -> u32` to src/inventory.rs, directly after `order_total`, returning the sum of all item quantities.
3. Bug: the bulk discount must apply when the quantity REACHES `BULK_THRESHOLD` (the test `bulk_discount_at_threshold` fails). Fix `apply_discount`.
4. Bug: `median()` in stats.py returns the wrong value for even-length input. Fix it.
5. Add `percentile(values, p)` to stats.py right after `median()`: p is in [0, 100], uses linear interpolation between closest ranks (like numpy's default), raises StatsError for empty input or p outside [0, 100].
6. Delete the deprecated `old_mean` function from stats.py and the README note that mentions it.
7. In README.md add rows for `total_quantity(items)` to the Rust API table and `percentile(values, p)` to the stats table.
8. In settings.ini set `workers = 8` and add a line `retries = 3` right after the `timeout = 30` line. Keep the file's CRLF line endings.
Do not change anything else. When you are done, reply with a short summary.";

const SYSTEM: &str = "You are ref, an AI coding agent running in the user's terminal. You operate on the user's machine through the provided tools and work autonomously until the task is done.
Every tool call takes a `purpose` argument: a few words, in the user's language, describing why you are making the call.";

struct Args {
    model: String,
    runs: usize,
    steps: usize,
    log: PathBuf,
    keep: bool,
    check: Option<PathBuf>,
    task: u8,
}

fn parse_args() -> Args {
    let mut a = Args {
        model: "zai/glm-5.3".into(),
        runs: 1,
        steps: 25,
        log: "/tmp/opencode/trials".into(),
        keep: false,
        check: None,
        task: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--model" => a.model = it.next().expect("--model value"),
            "--runs" => a.runs = it.next().and_then(|v| v.parse().ok()).expect("--runs N"),
            "--steps" => a.steps = it.next().and_then(|v| v.parse().ok()).expect("--steps N"),
            "--log" => a.log = it.next().expect("--log dir").into(),
            "--keep" => a.keep = true,
            "--task" => a.task = it.next().and_then(|v| v.parse().ok()).expect("--task 1|2"),
            "--check" => a.check = Some(it.next().expect("--check dir").into()),
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

/// Tool definitions as refd sends them: `purpose` first (optional here).
fn tool_defs() -> Vec<ref_tools::ToolDef> {
    ref_tools::builtin_defs()
        .into_iter()
        .map(|mut d| {
            if let Some(Value::Object(props)) = d.parameters.get("properties").cloned() {
                let mut np = serde_json::Map::new();
                np.insert(
                    "purpose".into(),
                    json!({"type": "string", "description": "Brief purpose of this call, shown to the user (in the user's language)"}),
                );
                np.extend(props);
                d.parameters["properties"] = Value::Object(np);
            }
            d
        })
        .collect()
}

fn setup_dir(dir: &Path) {
    for (name, content) in FIXTURES {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let content = if name.ends_with(".ini") {
            content.replace('\n', "\r\n")
        } else {
            content.to_string()
        };
        std::fs::write(p, content).unwrap();
    }
    std::fs::write(dir.join("src/lib.rs"), "pub mod cli;\npub mod inventory;\n").unwrap();
}

fn setup_dir2(dir: &Path) {
    std::fs::write(dir.join("handlers.py"), HANDLERS).unwrap();
}

/// External change made after the model's first look: a header comment at the top.
const EXTERNAL_LINE: &str = "# Auto-formatted by CI (do not remove this line)";

fn external_change2(dir: &Path) {
    let p = dir.join("handlers.py");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, format!("{EXTERNAL_LINE}\n{text}")).unwrap();
}

/// Expected result of task 2 computed from the fixture.
fn expected2() -> String {
    let mut out: Vec<String> = vec![EXTERNAL_LINE.to_string()];
    let lines: Vec<&str> = HANDLERS.lines().collect();
    let mut i = 0;
    let mut class = String::new();
    while i < lines.len() {
        let l = lines[i];
        if let Some(c) = l.strip_prefix("class ") {
            class = c.trim_end_matches(':').to_string();
        }
        if l == "class Handler31:" {
            // skip until next class
            i += 1;
            while i < lines.len() && !lines[i].starts_with("class ") {
                i += 1;
            }
            continue;
        }
        if l.trim_start().starts_with("retries = ")
            && ["Handler07", "Handler23", "Handler45", "Handler59"].contains(&class.as_str())
        {
            out.push("    retries = 10".into());
        } else if class == "Handler12" && l.trim_start().starts_with("return {\"name\"") {
            let (head, _) = l.rsplit_once("value * ").unwrap();
            out.push(format!("{head}value * 100}}"));
        } else if l.starts_with("    for cls in [") {
            out.push(l.replace("Handler31, ", ""));
        } else {
            out.push(l.to_string());
        }
        if class == "Handler50" && l.trim_start().starts_with("return f\"") {
            out.push(String::new());
            out.push("    def reset(self) -> None:".into());
            out.push("        self.retries = 0".into());
        }
        i += 1;
    }
    out.join("\n")
}

fn check2(dir: &Path) -> (usize, Vec<String>) {
    let got = std::fs::read_to_string(dir.join("handlers.py")).unwrap_or_default();
    let norm = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = s.lines().map(|l| l.trim_end().to_string()).collect();
        // Blank-line runs and trailing blanks are style, not content.
        v.dedup_by(|a, b| a.is_empty() && b.is_empty());
        while v.last().is_some_and(|l| l.is_empty()) {
            v.pop();
        }
        v
    };
    let mut want = norm(&expected2());
    let mut got_n = norm(&got);
    let mut ok = 0;
    let mut fails = vec![];
    // names() at the end: accept any reasonable body.
    let tail_ok = got.contains("def names() -> list[str]:") && got.contains("sorted(REGISTRY)");
    if tail_ok {
        ok += 1
    } else {
        fails.push("5: names() missing".to_string())
    }
    if let Some(p) = got_n.iter().position(|l| l.starts_with("def names(")) {
        got_n.truncate(p);
        while got_n.last().is_some_and(|l| l.is_empty()) {
            got_n.pop();
        }
    }
    while want.last().is_some_and(|l| l.is_empty()) {
        want.pop();
    }
    let sp = sh(
        "python3 -c 'import ast; ast.parse(open(\"handlers.py\").read())'",
        dir,
    );
    if sp.0 {
        ok += 1
    } else {
        fails.push(format!("syntax: {}", sp.1.lines().last().unwrap_or("")))
    }
    if got_n == want {
        ok += 1;
    } else {
        let (wr, gr): (Vec<&str>, Vec<&str>) = (
            want.iter().map(|x| x.as_str()).collect(),
            got_n.iter().map(|x| x.as_str()).collect(),
        );
        let d = similar::TextDiff::from_slices(&wr, &gr);
        let diff: Vec<String> = d
            .iter_all_changes()
            .filter(|c| c.tag() != similar::ChangeTag::Equal)
            .take(8)
            .map(|c| {
                format!(
                    "{}{}",
                    if c.tag() == similar::ChangeTag::Delete {
                        "-"
                    } else {
                        "+"
                    },
                    c.value()
                )
            })
            .collect();
        fails.push(format!(
            "content differs from expected: {}",
            diff.join(" | ")
        ));
    }
    if got.starts_with(EXTERNAL_LINE) {
        ok += 1
    } else {
        fails.push("external change lost".into())
    }
    (ok, fails)
}

/// Read-only bash for task 2: refuse commands that obviously modify files.
fn bash_blocked(cmd: &str) -> bool {
    let c = cmd
        .replace(">&", "")
        .replace("2>/dev/null", "")
        .replace(">/dev/null", "");
    c.contains('>')
        || [
            "sed -i", "perl -i", "python", "tee ", "mv ", "cp ", "rm ", "truncate", "dd ", "ed ",
        ]
        .iter()
        .any(|p| c.contains(p))
}

#[derive(Default, Debug)]
struct Stats {
    steps: usize,
    calls: usize,
    edit_calls: usize,
    edit_errors: usize,
    edit_warnings: usize,
    replace_calls: usize,
    other_errors: usize,
    bash_calls: usize,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
    llm_error: Option<String>,
    finished: bool,
}

fn now() -> i64 {
    ref_proto::now_ms()
}

fn text_part(text: String, meta: Option<Value>) -> Part {
    Part::Text(TextPart {
        id: ref_proto::new_id("prt"),
        text,
        time: now(),
        meta,
    })
}

/// One agent run. Returns stats; the transcript goes to `log`.
async fn run_once(
    http: &reqwest::Client,
    spec: &ref_llm::ModelSpec,
    dir: &Path,
    steps: usize,
    task: u8,
    log: &mut String,
) -> Stats {
    let mut st = Stats::default();
    let ctx = ref_tools::ToolCtx::new(dir);
    let mut sys = SYSTEM.to_string();
    sys.push_str(&format!(
        "\n<env>\nWorking directory: {}\nPlatform: linux\nGit repository: no\n</env>",
        dir.display()
    ));
    let mut user = Message::new(Role::User, MessageOrigin::User);
    user.parts.push(text_part(
        if task == 2 { TASK2 } else { TASK }.to_string(),
        None,
    ));
    let mut external_done = task != 2;
    let mut messages = vec![user];
    let tools = tool_defs();
    let session_id = ref_proto::new_id("ses");
    for step in 0..steps {
        st.steps = step + 1;
        let req = Request {
            system: vec![sys.clone()],
            messages: messages.clone(),
            tools: tools.clone(),
            session_id: session_id.clone(),
            tool_choice_none: false,
        };
        let mut attempt = 0;
        let mut events = loop {
            match ref_llm::stream(http, spec, &req).await {
                Ok(s) => break s,
                Err(e) if attempt < 3 => {
                    attempt += 1;
                    log.push_str(&format!("!! llm error (retrying): {e}\n"));
                    tokio::time::sleep(e.retry_after().unwrap_or(Duration::from_secs(5))).await;
                }
                Err(e) => {
                    st.llm_error = Some(e.to_string());
                    return st;
                }
            }
        };
        let mut msg = Message::new(Role::Assistant, MessageOrigin::User);
        msg.model = Some(spec.full_id());
        let mut texts: std::collections::BTreeMap<usize, (bool, String)> = Default::default();
        let mut calls: Vec<(String, String, Value, Option<Value>)> = vec![];
        let mut finish = None;
        while let Some(ev) = events.next().await {
            match ev {
                Err(e) => {
                    st.llm_error = Some(e.to_string());
                    log.push_str(&format!("!! stream error: {e}\n"));
                    return st;
                }
                Ok(StreamEvent::TextDelta { index, text }) => texts
                    .entry(index)
                    .or_insert((false, String::new()))
                    .1
                    .push_str(&text),
                Ok(StreamEvent::ReasoningDelta { index, text }) => texts
                    .entry(index)
                    .or_insert((true, String::new()))
                    .1
                    .push_str(&text),
                Ok(StreamEvent::TextEnd { index, meta }) => {
                    let t = texts.remove(&index).map(|x| x.1).unwrap_or_default();
                    log.push_str(&format!("--- assistant text:\n{t}\n"));
                    msg.parts.push(text_part(t, meta));
                }
                Ok(StreamEvent::ReasoningEnd { index, meta }) => {
                    let t = texts.remove(&index).map(|x| x.1).unwrap_or_default();
                    msg.parts.push(Part::Reasoning(ReasoningPart {
                        id: ref_proto::new_id("prt"),
                        text: t,
                        time: now(),
                        time_end: Some(now()),
                        meta,
                    }));
                }
                Ok(StreamEvent::ToolCallEnd {
                    call_id,
                    name,
                    input,
                    meta,
                    ..
                }) => calls.push((call_id, name, input, meta)),
                Ok(StreamEvent::Usage(u)) => {
                    st.input_tokens = st.input_tokens.max(u.input + u.cache_read + u.cache_write);
                    st.output_tokens += u.output;
                    st.cost += u.cost;
                }
                Ok(StreamEvent::Finish { reason }) => finish = Some(reason),
                Ok(_) => {}
            }
        }
        for (call_id, name, input, meta) in calls {
            st.calls += 1;
            let mut args = input.clone();
            if let Value::Object(m) = &mut args {
                m.remove("purpose");
            }
            log.push_str(&format!(
                "=== step {} call {name}: {}\n",
                step + 1,
                serde_json::to_string_pretty(&args).unwrap_or_default()
            ));
            if let Some(s) = args.get("input").and_then(Value::as_str) {
                log.push_str(&format!("--- edit input (raw):\n{s}\n"));
            }
            let t0 = Instant::now();
            let blocked = task == 2
                && name == "bash"
                && args
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(bash_blocked);
            let out = if blocked {
                ref_tools::ToolOutput::err(
                    "bash is read-only in this environment: use the edit tool to change files.",
                )
            } else {
                ref_tools::execute(&name, args.clone(), &ctx).await
            };
            if !external_done && matches!(name.as_str(), "read" | "grep") && !out.is_error {
                external_done = true;
                external_change2(dir);
                log.push_str("!! external change: inserted a line at the top of handlers.py\n");
            }
            let is_edit = name == "edit";
            if is_edit {
                st.edit_calls += 1;
                if args.get("old_string").is_some() {
                    st.replace_calls += 1;
                }
                if out.is_error {
                    st.edit_errors += 1;
                } else if out.text.contains("\nWarnings:\n") {
                    st.edit_warnings += 1;
                }
            } else if out.is_error {
                st.other_errors += 1;
            }
            if name == "bash" {
                st.bash_calls += 1;
            }
            log.push_str(&format!(
                "--- result ({}{}ms):\n{}\n",
                if out.is_error { "ERROR, " } else { "" },
                t0.elapsed().as_millis(),
                out.text
            ));
            msg.parts.push(Part::Tool(ToolPart {
                id: ref_proto::new_id("prt"),
                call_id,
                name,
                purpose: String::new(),
                input,
                input_raw: String::new(),
                state: if out.is_error {
                    ToolState::Error
                } else {
                    ToolState::Done
                },
                output: out.text,
                images: vec![],
                time: now(),
                time_start: None,
                time_end: None,
                run_at: None,
                meta,
            }));
        }
        let had_tools = msg.parts.iter().any(|p| matches!(p, Part::Tool(_)));
        msg.finish = Some(if had_tools {
            "tool_use".into()
        } else {
            "stop".into()
        });
        messages.push(msg);
        if !had_tools {
            st.finished = finish != Some(FinishReason::Error);
            break;
        }
    }
    st
}

const HIDDEN_RS: &str = r#"
#[cfg(test)]
mod hidden_tests {
    use super::*;
    #[test]
    fn hidden() {
        let items = vec![Item::new("a", "a", 100, 2), Item::new("b", "b", 100, 10)];
        assert_eq!(total_quantity(&items), 12);
        assert_eq!(total_quantity(&[]), 0);
        assert_eq!(order_total(&items), 200 + 850);
        assert_eq!(apply_discount(100, BULK_THRESHOLD - 1), 100);
        assert_eq!(apply_discount(100, BULK_THRESHOLD), 85);
    }
}
"#;

const HIDDEN_PY: &str = r#"
import sys
sys.path.insert(0, '.')
import stats
assert stats.median([1, 2, 3, 4]) == 2.5, stats.median([1, 2, 3, 4])
assert stats.median([5, 1, 3]) == 3
assert stats.median([4, 1]) == 2.5
p = stats.percentile
assert abs(p([1, 2, 3, 4], 50) - 2.5) < 1e-9, p([1, 2, 3, 4], 50)
assert abs(p([1, 2, 3, 4], 25) - 1.75) < 1e-9, p([1, 2, 3, 4], 25)
assert p([7], 90) == 7
assert p([3, 1, 2], 0) == 1 and p([3, 1, 2], 100) == 3
for bad in [([], 50), ([1], -1), ([1], 101)]:
    try:
        p(*bad)
    except stats.StatsError:
        pass
    else:
        raise AssertionError(f"no StatsError for {bad}")
assert not hasattr(stats, 'old_mean')
assert stats.mean([1, 2, 3]) == 2
print("py ok")
"#;

fn sh(cmd: &str, dir: &Path) -> (bool, String) {
    match std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
    {
        Ok(o) => (
            o.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// Lines of the original fixture that are allowed to change.
fn may_change(file: &str, line: &str) -> bool {
    let t = line.trim();
    match file {
        "src/inventory.rs" => {
            line.contains("calc_total")
                || t.starts_with("if quantity > BULK_THRESHOLD")
                || t.starts_with("if quantity >= BULK_THRESHOLD")
        }
        "src/cli.rs" => line.contains("calc_total"),
        "stats.py" => {
            t.starts_with("return (ordered[mid] + ordered[mid + 1]) / 2")
                || t.starts_with("def old_mean")
                || t.starts_with("# Deprecated: kept for the v1")
                || t == "total = 0"
                || t == "for v in values:"
                || t == "total += v"
                || t == "return total / len(values)"
        }
        "README.md" => line.contains("calc_total") || line.contains("old_mean"),
        "settings.ini" => t.starts_with("workers"),
        _ => false,
    }
}

/// Returns (passed checks, failed check descriptions).
fn check(dir: &Path) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut fails = vec![];
    let mut chk = |cond: bool, what: &str| {
        if cond {
            ok += 1;
        } else {
            fails.push(what.to_string());
        }
    };
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap_or_default();
    let inv = read("src/inventory.rs");
    let cli = read("src/cli.rs");
    let py = read("stats.py");
    let md = read("README.md");
    let ini_raw = std::fs::read(dir.join("settings.ini")).unwrap_or_default();
    let ini = String::from_utf8_lossy(&ini_raw).to_string();

    // 1. rename
    chk(
        !inv.contains("calc_total") && !cli.contains("calc_total") && !md.contains("calc_total"),
        "1: calc_total still present",
    );
    chk(
        inv.matches("pub fn order_total(").count() == 1,
        "1: exactly one `pub fn order_total`",
    );
    chk(
        cli.matches("order_total").count() >= 3,
        "1: cli.rs uses order_total (import + 2 calls)",
    );
    chk(
        md.matches("order_total").count() >= 2,
        "1: README mentions order_total twice",
    );
    // 2. total_quantity placement
    let pos = |s: &str, pat: &str| s.find(pat);
    chk(
        inv.matches("pub fn total_quantity(").count() == 1,
        "2: exactly one total_quantity",
    );
    chk(
        matches!((pos(&inv, "pub fn order_total("), pos(&inv, "pub fn total_quantity("), pos(&inv, "pub fn apply_discount(")),
            (Some(a), Some(b), Some(c)) if a < b && b < c),
        "2: total_quantity directly after order_total",
    );
    // 2/3 compile + hidden tests
    let build = dir.join(".check");
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(build.join("inventory.rs"), format!("{inv}\n{HIDDEN_RS}")).unwrap();
    std::fs::write(build.join("cli.rs"), &cli).unwrap();
    std::fs::write(build.join("lib.rs"), "pub mod cli;\npub mod inventory;\n").unwrap();
    let (built, out) = sh(
        "rustc --edition 2021 --test lib.rs -o t 2>&1 && ./t 2>&1",
        &build,
    );
    chk(
        built,
        &format!(
            "2/3: rust tests: {}",
            out.lines()
                .filter(|l| l.contains("error") || l.contains("panicked") || l.contains("FAILED"))
                .take(6)
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    );
    let _ = std::fs::remove_dir_all(&build);
    // 4/5/6 python
    let (pyok, out) = sh(&format!("python3 - <<'PYEOF'\n{HIDDEN_PY}\nPYEOF"), dir);
    chk(
        pyok,
        &format!("4/5/6: python checks: {}", out.lines().last().unwrap_or("")),
    );
    chk(
        matches!((pos(&py, "def median("), pos(&py, "def percentile("), pos(&py, "def mode(")), (Some(a), Some(b), Some(c)) if a < b && b < c),
        "5: percentile right after median",
    );
    chk(!py.contains("old_mean"), "6: old_mean removed");
    // 6/7 README
    chk(!md.contains("old_mean"), "6: README old_mean note removed");
    chk(
        md.lines()
            .any(|l| l.starts_with('|') && l.contains("total_quantity")),
        "7: README total_quantity row",
    );
    chk(
        md.lines()
            .any(|l| l.starts_with('|') && l.contains("percentile")),
        "7: README percentile row",
    );
    {
        let rust_tbl = md.find("## Rust API").unwrap_or(0);
        let stats_tbl = md.find("## 统计函数").unwrap_or(usize::MAX);
        let tq = md.find("total_quantity").unwrap_or(usize::MAX);
        let pc = md.find("percentile").unwrap_or(0);
        chk(
            tq > rust_tbl && tq < stats_tbl && pc > stats_tbl,
            "7: README rows in the right tables",
        );
    }
    // 8 ini
    let crlf_ok = !ini_raw.is_empty() && ini.matches("\r\n").count() == ini.matches('\n').count();
    chk(crlf_ok, "8: settings.ini keeps CRLF everywhere");
    let lines: Vec<&str> = ini.lines().map(|l| l.trim_end_matches('\r')).collect();
    chk(
        lines.iter().any(|l| l.replace(' ', "") == "workers=8")
            && !lines.iter().any(|l| l.replace(' ', "") == "workers=4"),
        "8: workers = 8",
    );
    chk(
        lines
            .iter()
            .position(|l| l.trim() == "timeout = 30")
            .is_some_and(|i| {
                lines
                    .get(i + 1)
                    .is_some_and(|n| n.replace(' ', "") == "retries=3")
            }),
        "8: retries = 3 right after timeout",
    );
    // Collateral damage: every original line not expected to change must still be there.
    for (name, orig) in FIXTURES {
        let now = if *name == "settings.ini" {
            ini.replace("\r\n", "\n")
        } else {
            read(name)
        };
        let mut avail: std::collections::HashMap<&str, isize> = Default::default();
        for l in now.lines() {
            *avail.entry(l).or_default() += 1;
        }
        let mut lost = vec![];
        for l in orig.lines() {
            if l.trim().is_empty() || may_change(name, l) {
                continue;
            }
            let c = avail.entry(l).or_default();
            *c -= 1;
            if *c < 0 {
                lost.push(l.to_string());
            }
        }
        chk(
            lost.is_empty(),
            &format!(
                "collateral: {name} lost/changed {} line(s): {:?}",
                lost.len(),
                lost.iter().take(3).collect::<Vec<_>>()
            ),
        );
    }
    (ok, fails)
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    if let Some(dir) = &args.check {
        let (passed, fails) = if args.task == 2 {
            check2(dir)
        } else {
            check(dir)
        };
        println!("checks {passed}/{}", passed + fails.len());
        for f in &fails {
            println!("    FAIL {f}");
        }
        return;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let config = ref_config::Config::load(Path::new(&home)).expect("load config");
    if args.model == "list" {
        for m in config.models() {
            println!("{}", m.id);
        }
        return;
    }
    let spec = config
        .resolve_model(&args.model, None)
        .expect("resolve model");
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    std::fs::create_dir_all(&args.log).unwrap();
    let mut totals = (0usize, 0usize, 0usize);
    for run in 0..args.runs {
        let dir = tempfile::Builder::new().prefix("trial-").tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        if args.task == 2 {
            setup_dir2(&root)
        } else {
            setup_dir(&root)
        }
        let mut log = String::new();
        let t0 = Instant::now();
        let st = run_once(&http, &spec, &root, args.steps, args.task, &mut log).await;
        let (passed, fails) = if args.task == 2 {
            check2(&root)
        } else {
            check(&root)
        };
        let total = passed + fails.len();
        let stamp = chrono_stamp();
        let name = format!(
            "{}-t{}-{stamp}-{run}.log",
            args.model.replace('/', "_"),
            args.task
        );
        log.push_str(&format!(
            "\n==== checks {passed}/{total}\n{}\n==== stats {st:?}\n",
            fails.join("\n")
        ));
        std::fs::write(args.log.join(&name), &log).unwrap();
        println!(
            "{} run {run}: checks {passed}/{total} | steps {} calls {} | edit {} (errors {}, warned {}, str_replace {}) | other errors {} | bash {} | {}s ${:.3}{}{} | log {}",
            args.model,
            st.steps,
            st.calls,
            st.edit_calls,
            st.edit_errors,
            st.edit_warnings,
            st.replace_calls,
            st.other_errors,
            st.bash_calls,
            t0.elapsed().as_secs(),
            st.cost,
            if st.finished { "" } else { " | NOT FINISHED" },
            st.llm_error.as_deref().map(|e| format!(" | llm error: {}", e.chars().take(200).collect::<String>())).unwrap_or_default(),
            args.log.join(&name).display()
        );
        for f in &fails {
            println!("    FAIL {f}");
        }
        totals.0 += st.edit_calls;
        totals.1 += st.edit_errors;
        totals.2 += usize::from(fails.is_empty());
        if args.keep {
            println!("    kept {}", dir.keep().display());
        }
    }
    println!(
        "TOTAL {}: {} / {} runs fully correct; edit calls {}, edit errors {}",
        args.model, totals.2, args.runs, totals.0, totals.1
    );
}

fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
