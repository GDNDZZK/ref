//! The scripts, by prompt word. Each submodule holds one family.

mod flow;
mod text;
mod tools;

use super::script::Script;

/// Every script, in the order `help` lists them.
pub fn all() -> Vec<&'static Script> {
    [text::SCRIPTS, tools::SCRIPTS, flow::SCRIPTS]
        .into_iter()
        .flatten()
        .collect()
}

pub fn find(name: &str) -> Option<&'static Script> {
    all().into_iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Ctx;

    pub(crate) fn ctx(prompt: &str, round: usize) -> Ctx {
        Ctx {
            prompt: prompt.into(),
            args: prompt.split_once(' ').map_or("", |(_, a)| a).into(),
            round,
            session_id: "ses_test".into(),
            attempt: 1,
            tool_results: vec![],
        }
    }

    #[test]
    fn names_are_unique_lowercase_words() {
        let names: Vec<&str> = all().iter().map(|s| s.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate script names");
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{name}"
            );
        }
    }

    #[test]
    fn every_script_has_a_first_round() {
        for script in all() {
            let round = (script.play)(&ctx(script.name, 0));
            assert!(
                !round.steps.is_empty(),
                "{} has no first round",
                script.name
            );
        }
    }
}
