//! JSONC (JSON with comments and trailing commas) parsing.

use serde_json::Value;

/// Parse JSONC text: `//` and `/* */` comments (outside strings) and trailing commas
/// before `}` / `]` are removed, then the result is parsed with serde_json
/// (object key order is preserved). Comments are replaced by spaces and newlines are
/// kept so that error positions still refer to the original text.
pub fn parse_jsonc(s: &str) -> anyhow::Result<Value> {
    let s = s.strip_prefix('\u{feff}').unwrap_or(s);
    let stripped = strip_comments(s);
    let cleaned = strip_trailing_commas(&stripped);
    Ok(serde_json::from_str(&cleaned)?)
}

fn strip_comments(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                out.push_str("  ");
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < chars.len() {
                    out.push_str("  ");
                    i += 2;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Assumes comments were already removed.
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                out.push(' ');
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comments_and_trailing_commas() {
        let v = parse_jsonc(
            r#"{
  // line comment
  "a": 1, /* block
  comment */ "b": [1, 2, 3,],
  "url": "http://x.y/z", // trailing
  "glob": "src/**/*.rs /* not a comment */",
  "esc": "quote \" // still string",
  "nested": {"x": true,},
}
"#,
        )
        .unwrap();
        assert_eq!(v["a"], json!(1));
        assert_eq!(v["b"], json!([1, 2, 3]));
        assert_eq!(v["url"], json!("http://x.y/z"));
        assert_eq!(v["glob"], json!("src/**/*.rs /* not a comment */"));
        assert_eq!(v["esc"], json!("quote \" // still string"));
        assert_eq!(v["nested"], json!({"x": true}));
    }

    #[test]
    fn preserves_key_order() {
        let v = parse_jsonc(r#"{"z": 1, "a": 2, "m": 3}"#).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["z", "a", "m"]);
    }

    #[test]
    fn errors_reported() {
        assert!(parse_jsonc("{\"a\": }").is_err());
        assert!(parse_jsonc("").is_err());
    }

    #[test]
    fn trailing_comma_with_comment_between() {
        let v = parse_jsonc("[1, 2, // c\n]").unwrap();
        assert_eq!(v, json!([1, 2]));
    }
}
