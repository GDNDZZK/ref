//! Sessions that exist before the first start, so lists and history views are not empty.

use crate::store::{Persisted, Store};
use ref_config::Config;
use ref_proto::{
    Message, MessageSource, ModelRef, Part, Role, SessionInfo, SessionStatus, ToolState, Usage,
    now_ms,
};
use serde_json::json;

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;
const MOCK_AGENT: &str = "build";
/// Context the old sessions report as used.
const USED_TOKENS: u64 = 12_000;

struct Builder {
    now: i64,
    messages: Vec<Message>,
}

impl Builder {
    fn user(mut self, text: &str) -> Builder {
        let time = self.now + self.messages.len() as i64 * MINUTE_MS;
        self.messages
            .push(message(Role::User, time, vec![text_part(text, time)]));
        self
    }

    fn assistant(mut self, parts: Vec<Part>) -> Builder {
        let time = self.now + self.messages.len() as i64 * MINUTE_MS;
        let mut m = message(Role::Assistant, time, parts);
        m.time_end = Some(time + MINUTE_MS / 2);
        m.model = Some(model());
        m.agent = Some(MOCK_AGENT.into());
        m.usage = Some(Usage {
            input: 1_200,
            output: 300,
            cache_read: 4_000,
            ..Usage::default()
        });
        self.messages.push(m);
        self
    }
}

fn model() -> ModelRef {
    ModelRef {
        provider: "mock".into(),
        model: "mock-1".into(),
        variant: None,
    }
}

fn message(role: Role, time: i64, parts: Vec<Part>) -> Message {
    Message {
        id: crate::util::new_id("msg"),
        role,
        time,
        time_end: None,
        source: MessageSource::Human,
        parts,
        model: None,
        agent: None,
        usage: None,
        error: None,
    }
}

fn text_part(text: &str, time: i64) -> Part {
    Part::Text {
        text: text.into(),
        time,
    }
}

fn tool_part(name: &str, purpose: &str, input: serde_json::Value, output: &str) -> Part {
    Part::Tool {
        call_id: format!("call_{}", crate::util::random_hex(6)),
        name: name.into(),
        purpose: purpose.into(),
        raw_input: input.to_string(),
        input,
        state: ToolState::Done,
        output: Some(output.into()),
        meta: None,
        images: vec![],
        time: 0,
        time_end: None,
        run_at: None,
        provider_data: None,
    }
}

struct Old {
    name: &'static str,
    title: &'static str,
    age_ms: i64,
    build: fn(Builder) -> Builder,
}

fn parser_fix(b: Builder) -> Builder {
    b.user("The parser panics on the last item. Can you look?")
        .assistant(vec![
            text_part("Let me read the parser.", 0),
            tool_part(
                "read",
                "Read the parser",
                json!({"path": "src/parser.rs"}),
                "[src/parser.rs#0000]\n(see the file)",
            ),
            text_part(
                "The loop in `parse_items` runs to `items.len()` inclusive, one past the end. Use `0..items.len()`.",
                0,
            ),
        ])
        .user("Thanks, I will fix it myself.")
        .assistant(vec![text_part("Sounds good. Tell me if the tests still fail.", 0)])
}

fn release_notes(b: Builder) -> Builder {
    b.user("Draft release notes for 0.3.")
        .assistant(vec![text_part(
            "## 0.3\n\n- **Parser**: no more panic on empty input.\n- **CLI**: `--verbose` prints timings.\n- **Docs**: the design notes moved to `docs/design.md`.",
            0,
        )])
}

fn bench_ideas(b: Builder) -> Builder {
    b.user("What should we benchmark first?")
        .assistant(vec![text_part(
            "Start with `parse_items` on a large input: it is on the hot path and easy to measure.",
            0,
        )])
}

const OLD_SESSIONS: [Old; 3] = [
    Old {
        name: "parser-fix",
        title: "Fix parser panic",
        age_ms: DAY_MS,
        build: parser_fix,
    },
    Old {
        name: "release-notes",
        title: "Draft release notes",
        age_ms: 3 * DAY_MS,
        build: release_notes,
    },
    Old {
        name: "bench-ideas",
        title: "What to benchmark",
        age_ms: 9 * DAY_MS,
        build: bench_ideas,
    },
];

/// Saves the old sessions into `store`, all working in `cwd`.
pub fn save(store: &Store, cfg: &Config, cwd: &str) -> Result<(), String> {
    for old in OLD_SESSIONS {
        let created = now_ms() - old.age_ms;
        let builder = (old.build)(Builder {
            now: created,
            messages: vec![],
        });
        let mut context = crate::models::context_for(cfg, &model());
        context.used = USED_TOKENS;
        let info = SessionInfo {
            id: crate::util::new_id("ses"),
            name: old.name.into(),
            title: old.title.into(),
            agent: MOCK_AGENT.into(),
            model: model(),
            cwd: cwd.into(),
            created_at: created,
            updated_at: created + builder.messages.len() as i64 * MINUTE_MS,
            status: SessionStatus::Idle,
            attached: 0,
            live: false,
            parent: None,
            must_report: false,
            context,
            todos: vec![],
            busy_since: None,
            last_error: None,
        };
        store.save(Persisted {
            info,
            messages: builder.messages,
            reported: false,
            context_notice: false,
        });
    }
    store.flush()
}
