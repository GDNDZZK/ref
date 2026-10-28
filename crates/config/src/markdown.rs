//! Markdown files with YAML frontmatter (agents, commands, skills).

use serde_json::{Map, Value};

/// Split `---\n<yaml>\n---\n<body>`. Returns (frontmatter object, body). Files without
/// frontmatter yield an empty map and the whole text as body. Returns `Err` when the
/// frontmatter exists but is not valid YAML (even after sanitizing unquoted colons).
pub fn parse(text: &str) -> Result<(Map<String, Value>, String), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some((yaml, body)) = split(text) else {
        return Ok((Map::new(), text.to_string()));
    };
    let parsed = parse_yaml(yaml).or_else(|e| parse_yaml(&sanitize(yaml)).map_err(|_| e))?;
    Ok((parsed, body.to_string()))
}

fn split(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    // Closing delimiter: a line that is exactly `---`.
    let mut pos = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &rest[..pos];
            let body = &rest[pos + line.len()..];
            return Some((yaml, body));
        }
        pos += line.len();
    }
    // Unterminated frontmatter: `---` at EOF without newline.
    if rest[pos..].trim_end() == "---" {
        return Some((&rest[..pos], ""));
    }
    None
}

fn parse_yaml(yaml: &str) -> Result<Map<String, Value>, String> {
    if yaml.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_yaml::from_str::<Value>(yaml) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(Value::Null) => Ok(Map::new()),
        Ok(_) => Err("frontmatter is not a mapping".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Other agents accept unquoted colons in frontmatter values (`description: a: b`); rewrite such
/// values as block scalars (same approach as opencode).
fn sanitize(yaml: &str) -> String {
    let mut out = Vec::new();
    for line in yaml.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.is_empty() || line.starts_with([' ', '\t']) {
            out.push(line.to_string());
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            out.push(line.to_string());
            continue;
        };
        let valid_key = !key.is_empty()
            && key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let value = value.trim();
        if !valid_key
            || value.is_empty()
            || value == ">"
            || value == "|"
            || value.starts_with('"')
            || value.starts_with('\'')
            || !value.contains(':')
        {
            out.push(line.to_string());
            continue;
        }
        out.push(format!("{}: |-", key.trim_end()));
        out.push(format!("  {value}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter() {
        let (fm, body) = parse("---\ndescription: Reviews code\nmode: subagent\n---\nYou review.\n").unwrap();
        assert_eq!(fm["description"], "Reviews code");
        assert_eq!(fm["mode"], "subagent");
        assert_eq!(body, "You review.\n");

        let (fm, body) = parse("no frontmatter").unwrap();
        assert!(fm.is_empty());
        assert_eq!(body, "no frontmatter");

        let (fm, _) = parse("---\r\ndescription: Use when: things happen\r\n---\r\nx").unwrap();
        assert_eq!(fm["description"], "Use when: things happen");

        let (fm, body) = parse("---\n---\nbody").unwrap();
        assert!(fm.is_empty());
        assert_eq!(body, "body");
    }
}
