//! The scripts, by prompt word. Each submodule holds one family.

mod flow;
mod orchestrate;
mod question;
mod text;
mod tools;

use super::script::Script;

/// Every script, in the order `help` lists them.
pub fn all() -> Vec<&'static Script> {
    [
        text::SCRIPTS,
        tools::SCRIPTS,
        flow::SCRIPTS,
        orchestrate::SCRIPTS,
        question::SCRIPTS,
    ]
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
            root_prompt: prompt.into(),
            peer_reports: 0,
            typed: vec![],
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
            let answers = !round.steps.is_empty() || round.error.is_some();
            assert!(answers, "{} has no first round", script.name);
        }
    }

    /// Checks `value` against the parts of JSON Schema the tool definitions use.
    fn conforms(schema: &serde_json::Value, value: &serde_json::Value) -> Result<(), String> {
        use serde_json::Value;
        let kind = schema["type"].as_str().unwrap_or("");
        let ok = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            _ => true,
        };
        if !ok {
            return Err(format!("expected {kind}, got {value}"));
        }
        if let (Some(allowed), Value::String(_)) = (schema["enum"].as_array(), value)
            && !allowed.contains(value)
        {
            return Err(format!("{value} is not one of {allowed:?}"));
        }
        if let Value::Object(map) = value {
            for key in schema["required"].as_array().into_iter().flatten() {
                let key = key.as_str().unwrap_or("");
                if !map.contains_key(key) {
                    return Err(format!("missing {key}"));
                }
            }
            for (key, v) in map {
                match schema["properties"].get(key) {
                    Some(sub) => conforms(sub, v).map_err(|e| format!("{key}: {e}"))?,
                    None if schema["additionalProperties"] == false => {
                        return Err(format!("unknown property {key}"));
                    }
                    None => {}
                }
            }
        }
        if let Value::Array(items) = value {
            for v in items {
                conforms(&schema["items"], v)?;
            }
        }
        Ok(())
    }

    /// Every tool call of every script, in every round, fits the real tool definition.
    #[test]
    fn tool_calls_fit_the_tool_schemas() {
        use crate::mock::script::Step;
        let mut cfg = ref_config::Config::default();
        cfg.agents.insert(
            "build".into(),
            ref_config::AgentConfig {
                name: "build".into(),
                can_spawn: true,
                ..Default::default()
            },
        );
        let defs = crate::toolset::assemble(&cfg, "build", &[]);
        let read = defs.iter().find(|e| e.def.name == "read").unwrap();
        let missing = serde_json::json!({"purpose": "x"});
        assert!(conforms(&read.def.parameters, &missing).is_err());
        let mut checked = 0;
        for script in all() {
            for round in 0..12 {
                for reports in 0..=5 {
                    let mut c = ctx(script.name, round);
                    c.peer_reports = reports;
                    c.args = "ses_main".into();
                    c.root_prompt = "orchestrate".into();
                    for step in (script.play)(&c).steps {
                        let Step::Tool(call) = step else { continue };
                        let entry = defs
                            .iter()
                            .find(|e| e.def.name == call.name)
                            .unwrap_or_else(|| panic!("no tool {}", call.name));
                        let args: serde_json::Value =
                            serde_json::from_str(&call.raw_arguments()).unwrap();
                        let verdict = conforms(&entry.def.parameters, &args);
                        assert!(
                            verdict.is_ok(),
                            "{} round {round}: {} {args}: {verdict:?}",
                            script.name,
                            call.name
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }
}
