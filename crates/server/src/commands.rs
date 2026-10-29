//! Custom command expansion and markdown export.

use hn_proto::*;

/// Split command input into args: quoted strings stay together (quotes trimmed).
pub fn split_args(input: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?:\[Image\s+\d+\]|"[^"]*"|'[^']*'|[^\s"']+)"#).expect("static regex");
    re.find_iter(input)
        .map(|m| {
            let s = m.as_str();
            let quoted = s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')));
            if quoted {
                s[1..s.len() - 1].to_string()
            } else {
                s.to_string()
            }
        })
        .collect()
}

/// opencode template expansion: `$ARGUMENTS`, `$1..$N` (highest swallows the rest);
/// without placeholders, non-empty args are appended after a blank line.
pub fn expand_template(template: &str, args: &str) -> String {
    let re = regex::Regex::new(r"\$(\d+)").expect("static regex");
    let argv = split_args(args);
    let max = re.captures_iter(template).filter_map(|c| c[1].parse::<usize>().ok()).filter(|n| *n > 0).max().unwrap_or(0);
    let has_args = template.contains("$ARGUMENTS");
    let mut out = re
        .replace_all(template, |c: &regex::Captures| {
            let n: usize = c[1].parse().unwrap_or(0);
            if n == 0 {
                return c[0].to_string();
            }
            if n == max {
                argv.get(n - 1..).map(|r| r.join(" ")).unwrap_or_default()
            } else {
                argv.get(n - 1).cloned().unwrap_or_default()
            }
        })
        .into_owned();
    out = out.replace("$ARGUMENTS", args);
    if max == 0 && !has_args && !args.trim().is_empty() {
        out = format!("{}\n\n{}", out.trim_end(), args.trim());
    }
    out
}

pub fn export_markdown(info: &SessionInfo, msgs: &[Message]) -> String {
    let mut out = format!(
        "# {}\n\n- Session: `{}`\n- Agent: {}\n- Model: {}\n- Directory: `{}`\n- Created: {}\n\n---\n\n",
        info.name,
        info.id,
        info.agent,
        info.model,
        info.cwd,
        crate::util::fmt_time(info.created_at)
    );
    for m in msgs {
        let who = match (&m.role, &m.origin) {
            (Role::Assistant, _) => format!("## Assistant ({})", m.agent.as_deref().unwrap_or("")),
            (_, Origin::Human) => "## User".to_string(),
            (_, Origin::Peer { from_name, from_id }) => format!("## Message from {from_name} ({from_id})"),
            (_, Origin::Spawn { from_name, from_id }) => format!("## Task from {from_name} ({from_id})"),
            (_, Origin::System) => "## System".to_string(),
            (_, Origin::Compaction) => "## Compaction checkpoint".to_string(),
        };
        out.push_str(&format!("{who}\n\n_{}_\n\n", crate::util::fmt_time(m.time)));
        for p in &m.parts {
            match p {
                Part::Text { text, .. } => {
                    out.push_str(text.trim_end());
                    out.push_str("\n\n");
                }
                Part::Reasoning { text, .. } if !text.trim().is_empty() => {
                    out.push_str("<details><summary>Thinking</summary>\n\n");
                    out.push_str(text.trim_end());
                    out.push_str("\n\n</details>\n\n");
                }
                Part::Reasoning { .. } => {}
                Part::Tool(tp) => {
                    let purpose = if tp.purpose.is_empty() { String::new() } else { format!(" — {}", tp.purpose) };
                    out.push_str(&format!("**Tool `{}`**{purpose} ({:?})\n\n", tp.name, tp.state));
                    let input = if tp.input.is_null() { tp.raw_input.clone() } else { serde_json::to_string_pretty(&tp.input).unwrap_or_default() };
                    out.push_str(&format!("```json\n{}\n```\n\n", input.trim()));
                    if !tp.output.is_empty() {
                        let (o, cut) = crate::util::truncate_chars(tp.output.trim_end(), 20_000);
                        out.push_str(&format!("```\n{o}{}\n```\n\n", if cut { "\n[… truncated]" } else { "" }));
                    }
                }
                Part::Image(img) => out.push_str(&format!("_[image {}]_\n\n", img.mime)),
            }
        }
        if let Some(e) = &m.error {
            out.push_str(&format!("> **Error:** {e}\n\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_templates() {
        assert_eq!(expand_template("Review $1 then $2", "a b c d"), "Review a then b c d");
        assert_eq!(expand_template("Do: $ARGUMENTS", "x \"y z\""), "Do: x \"y z\"");
        assert_eq!(expand_template("Plain", "extra args"), "Plain\n\nextra args");
        assert_eq!(expand_template("Plain", ""), "Plain");
        assert_eq!(expand_template("$1|$2", "\"a b\""), "a b|");
    }
}
