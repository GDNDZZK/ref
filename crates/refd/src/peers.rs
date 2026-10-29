//! Inter-session messaging helpers: target resolution and message formatting.

/// Maximum peer message size in bytes.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}

/// Resolve `to` (exact id, else exact name, else case-insensitive name) among `open`
/// sessions. `closed` are history sessions (for a better error message). The result may be
/// `self_id`; callers decide whether that's allowed.
pub fn resolve(to: &str, open: &[Candidate], closed: &[Candidate]) -> Result<String, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("Error: target session must not be empty.".into());
    }
    if let Some(c) = open.iter().find(|c| c.id == to) {
        return Ok(c.id.clone());
    }
    if let Some(c) = closed.iter().find(|c| c.id == to) {
        return Err(format!(
            "Error: session \"{}\" ({}) is closed (not loaded). Only open sessions can be addressed.",
            c.name, c.id
        ));
    }
    let pick = |matches: Vec<&Candidate>| -> Option<Result<String, String>> {
        match matches.len() {
            0 => None,
            1 => Some(Ok(matches[0].id.clone())),
            _ => {
                let list: Vec<String> = matches.iter().map(|c| format!("\"{}\" (id {})", c.name, c.id)).collect();
                Some(Err(format!("Error: \"{to}\" is ambiguous. Candidates: {}. Use an id.", list.join(", "))))
            }
        }
    };
    if let Some(r) = pick(open.iter().filter(|c| c.name == to).collect()) {
        return r;
    }
    let lower = to.to_lowercase();
    if let Some(r) = pick(open.iter().filter(|c| c.name.to_lowercase() == lower).collect()) {
        return r;
    }
    if let Some(c) = closed.iter().find(|c| c.name == to || c.name.to_lowercase() == lower) {
        return Err(format!(
            "Error: session \"{}\" ({}) is closed (not loaded). Only open sessions can be addressed.",
            c.name, c.id
        ));
    }
    let names: Vec<String> = open.iter().map(|c| format!("\"{}\"", c.name)).collect();
    Err(format!(
        "Error: no open session named \"{to}\". Open sessions: {}",
        if names.is_empty() { "(none)".to_string() } else { names.join(", ") }
    ))
}

/// Text of a peer message as delivered to the target model.
pub fn wrap_peer_message(from_name: &str, from_id: &str, from_cwd: &str, text: &str) -> String {
    format!(
        "[Message from session \"{from_name}\" (id: {from_id}, cwd: {from_cwd})]\n{text}\n\n(Reply with send_message to \"{from_id}\" if a response is needed.)"
    )
}

pub fn reminder_text(parent_name: &str, parent_id: &str) -> String {
    format!(
        "Reminder: you were asked to report back to session \"{parent_name}\" (id {parent_id}) with send_message before stopping. Send your report now."
    )
}

/// Pick a name not in `taken`: `base`, `base (2)`, `base (3)`, ...
pub fn unique_name(base: &str, taken: &[String]) -> String {
    let base = base.trim();
    let base = if base.is_empty() { "session" } else { base };
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    for n in 2u32.. {
        let cand = format!("{base} ({n})");
        if !taken.contains(&cand) {
            return cand;
        }
    }
    unreachable!()
}

/// Sanitize a user/model supplied session name: single line, trimmed, max 100 chars.
pub fn clean_name(name: &str) -> String {
    let one: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = one.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(100).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, name: &str) -> Candidate {
        Candidate { id: id.into(), name: name.into() }
    }

    #[test]
    fn resolution() {
        let open = vec![c("ses_a", "Alpha"), c("ses_b", "beta"), c("ses_c", "Beta"), c("ses_d", "前端")];
        let closed = vec![c("ses_z", "Old")];
        assert_eq!(resolve("ses_a", &open, &closed).unwrap(), "ses_a");
        assert_eq!(resolve("alpha", &open, &closed).unwrap(), "ses_a");
        assert_eq!(resolve("beta", &open, &closed).unwrap(), "ses_b");
        assert_eq!(resolve("前端", &open, &closed).unwrap(), "ses_d");
        let e = resolve("BETA", &open, &closed).unwrap_err();
        assert!(e.contains("ambiguous") && e.contains("ses_b") && e.contains("ses_c"), "{e}");
        assert!(resolve("old", &open, &closed).unwrap_err().contains("closed"));
        assert!(resolve("ses_z", &open, &closed).unwrap_err().contains("closed"));
        assert!(resolve("nobody", &open, &closed).unwrap_err().contains("no open session"));
    }

    #[test]
    fn names() {
        let taken = vec!["a".to_string(), "a (2)".to_string()];
        assert_eq!(unique_name("a", &taken), "a (3)");
        assert_eq!(unique_name("b", &taken), "b");
        assert_eq!(clean_name("  x\n y\t "), "x y");
        let w = wrap_peer_message("n", "ses_1", "/w", "hello");
        assert!(w.starts_with("[Message from session \"n\" (id: ses_1, cwd: /w)]\nhello\n\n(Reply"));
    }
}
