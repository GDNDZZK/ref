//! The configuration of mock mode. User and project config files are never read.

use ref_config::Config;
use serde_json::{Value, json};
use std::path::Path;

const MOCK_1: &str = "mock-1";
const MOCK_FAST: &str = "mock-fast";
const MOCK_1_CONTEXT: u64 = 200_000;
const MOCK_FAST_CONTEXT: u64 = 64_000;

/// The context window of a mock model.
pub fn context_window(model: &str) -> Option<u64> {
    match model {
        MOCK_1 => Some(MOCK_1_CONTEXT),
        MOCK_FAST => Some(MOCK_FAST_CONTEXT),
        _ => None,
    }
}

/// Pre-execution delay of `delete`, long enough to watch the countdown.
const DELETE_DELAY_SECS: u32 = 5;

/// Config document (opencode syntax) for a mock LLM listening on `llm_port`.
fn document(llm_port: u16) -> Value {
    let variants = json!({"low": {}, "medium": {}, "high": {}});
    json!({
        "model": format!("mock/{MOCK_1}"),
        "provider": {
            "mock": {
                "name": "Mock",
                "api": "chat",
                "options": {
                    "baseURL": format!("http://127.0.0.1:{llm_port}"),
                    "apiKey": "mock",
                },
                "models": {
                    MOCK_1: {
                        "name": "Mock 1",
                        "limit": {"context": MOCK_1_CONTEXT, "output": 32_000},
                        "modalities": {"input": ["text", "image"]},
                        "reasoning": true,
                        "variants": variants,
                    },
                    MOCK_FAST: {
                        "name": "Mock Fast",
                        "limit": {"context": MOCK_FAST_CONTEXT, "output": 8_000},
                    },
                },
            },
        },
        // Reads are free, edits and writes need approval except in scratch/, and webfetch is
        // switched off (the mock never goes online). `curl` is refused per call, which shows
        // the Denied state; a tool that is denied outright would not exist for the model at all.
        // The last matching rule wins, and `edit` also covers `write` and `delete`.
        "permission": {
            "edit": {"*": "ask", "scratch/*": "allow"},
            "delete": "allow",
            "bash": {
                "*": "ask",
                "echo*": "allow",
                "ls*": "allow",
                "cat*": "allow",
                "seq*": "allow",
                "sleep*": "allow",
                "printf*": "allow",
                "curl*": "deny",
            },
            "webfetch": "deny",
        },
        "delay": {"delete": DELETE_DELAY_SECS},
        "agent": {
            "reviewer": {
                "description": "Reviews changes and reports findings",
                "creatable": true,
                "permission": {"edit": "deny", "write": "deny", "delete": "deny"},
            },
        },
    })
}

/// Loads the mock config through the normal loader from a file in `config_dir`, with an empty
/// environment and no home directory, so nothing outside leaks in.
pub fn load(config_dir: &Path, workspace: &Path, llm_port: u16) -> std::io::Result<Config> {
    std::fs::create_dir_all(config_dir)?;
    let text = serde_json::to_string_pretty(&document(llm_port)).expect("config serializes");
    std::fs::write(config_dir.join("ref.json"), text)?;
    let dirs = [config_dir.to_path_buf()];
    Ok(ref_config::load_with(None, &dirs, workspace, &|_| None))
}
