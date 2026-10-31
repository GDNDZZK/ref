//! The `question` tool: the model asks the user one or more questions and waits.
//!
//! It works like a permission request (see `engine::run_one`): the call registers a pending
//! question on the session, the session shows `WaitingQuestion`, the clients get a `Question`
//! event (and a notification), and the first `QuestionReply` ends the wait. The model gets the
//! answers as the tool result; a dismissed question ends as a denied call, an interrupt as a
//! cancelled one.
//!
//! The parameters follow opencode's `question` tool; `default` on an option is ours.

use crate::engine::{finish_tool, set_state};
use crate::hub::Hub;
use crate::session::{PendingQuestion, Session};
use crate::toolset::def;
use ref_llm::ToolDef;
use ref_proto::{
    NotifyKind, Question, QuestionRequest, SessionEvent, SessionStatus, ToolState, now_ms,
};
use ref_tools::ToolOutput;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub const TOOL: &str = "question";

const MAX_QUESTIONS: usize = 4;
/// One more row (the custom answer) still fits the number keys 1 to 9.
const MAX_OPTIONS: usize = 8;
const MAX_HEADER_CHARS: usize = 30;
const MAX_ANSWER_CHARS: usize = 2000;
const NOTIFY_DETAIL_CHARS: usize = 100;
const UNANSWERED: &str = "Unanswered";
const DISMISSED_OUTPUT: &str = "The user dismissed the question without answering.";
const NO_PENDING: &str = "No pending question with that id.";

/// The reply of a client: the answers per question, or `None` for "dismissed".
pub type Reply = Option<Vec<Vec<String>>>;

const DESCRIPTION: &str = "Ask the user one or more questions during execution and wait for the answers. Use it to gather preferences or requirements, clarify ambiguous instructions, get decisions on implementation choices, or offer choices about what direction to take.

Usage notes:
- When `custom` is enabled (the default), a \"Type your own answer\" option is added automatically; don't include \"Other\" or catch-all options
- Answers come back as the labels the user picked or the text they typed; set `multiple: true` to allow more than one
- Mark the option you recommend with `default: true`; it is preselected. In a single choice mark at most one
- Ask at most 4 questions per call, each with at most 8 options
- The user may dismiss the questions; continue without them then";

pub fn definition() -> ToolDef {
    let option = json!({"type": "object", "properties": {
        "label": {"type": "string", "description": "Display text (1-5 words, concise)"},
        "description": {"type": "string", "description": "Explanation of choice"},
        "default": {"type": "boolean", "description": "Preselected; the recommended choice"}
    }, "required": ["label", "description"]});
    let question = json!({"type": "object", "properties": {
        "question": {"type": "string", "description": "Complete question"},
        "header": {"type": "string", "description": "Very short label (max 30 chars)"},
        "options": {"type": "array", "description": "Available choices", "items": option},
        "multiple": {"type": "boolean", "description": "Allow selecting multiple choices"},
        "custom": {"type": "boolean", "description": "Allow typing a custom answer (default: true)"}
    }, "required": ["question", "header", "options"]});
    def(
        TOOL,
        DESCRIPTION,
        json!({"type": "object", "properties": {
            "questions": {"type": "array", "description": "Questions to ask", "items": question}
        }, "required": ["questions"]}),
    )
}

/// The questions of a call, checked. Errors go back to the model as the tool result.
pub fn parse(input: &Value) -> Result<Vec<Question>, String> {
    let list = input
        .get("questions")
        .ok_or("Error: `questions` is required.")?;
    let questions: Vec<Question> = serde_json::from_value(list.clone())
        .map_err(|e| format!("Error: invalid questions: {e}"))?;
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "Error: ask 1 to {MAX_QUESTIONS} questions per call."
        ));
    }
    for (i, q) in questions.iter().enumerate() {
        let n = i + 1;
        if q.question.trim().is_empty() {
            return Err(format!("Error: question {n} is empty."));
        }
        if q.header.trim().is_empty() || q.header.chars().count() > MAX_HEADER_CHARS {
            return Err(format!(
                "Error: the header of question {n} must be 1 to {MAX_HEADER_CHARS} characters."
            ));
        }
        if q.options.len() > MAX_OPTIONS {
            return Err(format!(
                "Error: question {n} has more than {MAX_OPTIONS} options."
            ));
        }
        if q.options.is_empty() && !q.custom {
            return Err(format!(
                "Error: question {n} has no options and no custom answer."
            ));
        }
        let labels: Vec<&str> = q.options.iter().map(|o| o.label.trim()).collect();
        if labels.iter().any(|l| l.is_empty()) {
            return Err(format!(
                "Error: question {n} has an option without a label."
            ));
        }
        if (1..labels.len()).any(|k| labels[..k].contains(&labels[k])) {
            return Err(format!("Error: question {n} repeats an option label."));
        }
        let defaults = q.options.iter().filter(|o| o.default).count();
        if !q.multiple && defaults > 1 {
            return Err(format!(
                "Error: question {n} is a single choice with {defaults} default options."
            ));
        }
    }
    Ok(questions)
}

/// Checks a client's answers and trims them: one entry per question, one choice unless
/// `multiple`, and only listed labels unless `custom`. An empty entry means "not answered".
pub fn check_answers(
    questions: &[Question],
    answers: Vec<Vec<String>>,
) -> Result<Vec<Vec<String>>, String> {
    if answers.len() != questions.len() {
        return Err(format!(
            "Expected {} answers, got {}.",
            questions.len(),
            answers.len()
        ));
    }
    let mut out = Vec::new();
    for (i, (q, entry)) in questions.iter().zip(answers).enumerate() {
        let n = i + 1;
        let entry: Vec<String> = entry
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if !q.multiple && entry.len() > 1 {
            return Err(format!("Question {n} takes one answer."));
        }
        for a in &entry {
            if a.chars().count() > MAX_ANSWER_CHARS {
                return Err(format!("The answer to question {n} is too long."));
            }
            let listed = q.options.iter().any(|o| o.label.trim() == a);
            if !q.custom && !listed {
                return Err(format!("Question {n} takes only the listed options."));
            }
        }
        out.push(entry);
    }
    Ok(out)
}

/// The tool result for the model.
pub fn format_answers(questions: &[Question], answers: &[Vec<String>]) -> String {
    let pairs: Vec<String> = questions
        .iter()
        .zip(answers)
        .map(|(q, a)| {
            let text = if a.is_empty() {
                UNANSWERED.to_string()
            } else {
                a.join(", ")
            };
            format!("\"{}\"=\"{}\"", q.question, text)
        })
        .collect();
    format!(
        "User has answered your questions: {}. You can now continue with the user's answers in mind.",
        pairs.join(", ")
    )
}

/// Delivers a client's reply to the waiting call. The first reply wins; an invalid one is
/// refused and the question stays open.
pub fn reply(s: &Session, call_id: &str, answers: Reply) -> Result<(), String> {
    let mut st = s.lock();
    let pending = st
        .questions
        .iter_mut()
        .find(|p| p.request.call_id == call_id && p.tx.is_some())
        .ok_or(NO_PENDING)?;
    let answers = match answers {
        Some(a) => Some(check_answers(&pending.request.questions, a)?),
        None => None,
    };
    if let Some(tx) = pending.tx.take() {
        let _ = tx.send(answers);
    }
    Ok(())
}

/// Tells the clients that every pending question of the session is over (run ended, abort).
pub fn clear(hub: &Hub, s: &Session, st: &mut crate::session::SessionState) {
    for p in std::mem::take(&mut st.questions) {
        hub.emit(
            &s.id,
            SessionEvent::QuestionResolved {
                call_id: p.request.call_id,
            },
        );
    }
}

/// What the call at `index` of message `msg_id` does: asks, waits, and finishes the tool part.
pub async fn run(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    msg_id: &str,
    index: usize,
    call_id: &str,
    input: &Value,
    cancel: &CancellationToken,
) {
    let questions = match parse(input) {
        Ok(q) => q,
        Err(e) => {
            finish_tool(hub, s, msg_id, index, ToolState::Error, ToolOutput::err(e));
            return;
        }
    };
    let (tx, rx) = oneshot::channel();
    {
        let mut st = s.lock();
        let request = QuestionRequest {
            session_id: s.id.clone(),
            message_id: msg_id.to_string(),
            call_id: call_id.to_string(),
            questions: questions.clone(),
            time: now_ms(),
        };
        let detail = crate::util::truncate_chars(&questions[0].question, NOTIFY_DETAIL_CHARS);
        st.questions.push(PendingQuestion {
            request: request.clone(),
            tx: Some(tx),
        });
        hub.set_status(&mut st, SessionStatus::WaitingQuestion);
        hub.emit(&s.id, SessionEvent::Question { request });
        hub.notify(&st, NotifyKind::Question, Some(detail));
    }
    set_state(hub, s, msg_id, index, ToolState::Running, None);
    let reply = tokio::select! {
        r = rx => r.ok(),
        _ = cancel.cancelled() => None,
    };
    {
        let mut st = s.lock();
        let before = st.questions.len();
        st.questions.retain(|p| p.request.call_id != call_id);
        if st.questions.len() < before {
            hub.emit(
                &s.id,
                SessionEvent::QuestionResolved {
                    call_id: call_id.to_string(),
                },
            );
        }
        if !cancel.is_cancelled() {
            hub.set_status(&mut st, SessionStatus::Running);
        }
    }
    if cancel.is_cancelled() {
        let out = ToolOutput::err(crate::engine::CANCELLED_OUTPUT);
        finish_tool(hub, s, msg_id, index, ToolState::Cancelled, out);
        return;
    }
    match reply {
        Some(Some(answers)) => {
            let mut out = ToolOutput::ok(format_answers(&questions, &answers));
            out.meta = Some(json!({ "answers": answers }));
            finish_tool(hub, s, msg_id, index, ToolState::Done, out);
        }
        _ => {
            let out = ToolOutput::err(DISMISSED_OUTPUT);
            finish_tool(hub, s, msg_id, index, ToolState::Denied, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(questions: Value) -> Value {
        json!({ "questions": questions })
    }

    fn one(extra: Value) -> Value {
        let mut q = json!({
            "question": "Which one?",
            "header": "Pick",
            "options": [
                {"label": "A", "description": "first"},
                {"label": "B", "description": "second", "default": true},
            ],
        });
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        input(json!([q]))
    }

    #[test]
    fn a_valid_call_parses_with_defaults() {
        let q = parse(&one(json!({}))).unwrap();
        assert_eq!(q.len(), 1);
        assert!(q[0].custom && !q[0].multiple);
        assert!(!q[0].options[0].default && q[0].options[1].default);
    }

    #[test]
    fn invalid_calls_say_what_is_wrong() {
        let valid = one(json!({}))["questions"][0].clone();
        let cases = [
            (json!({}), "required"),
            (input(json!([])), "1 to 4"),
            (input(json!(vec![valid.clone(); 5])), "1 to 4"),
            (input(json!("x")), "invalid questions"),
            (one(json!({"header": ""})), "header"),
            (one(json!({"header": "x".repeat(31)})), "header"),
            (one(json!({"question": " "})), "is empty"),
            (one(json!({"options": [], "custom": false})), "no options"),
            (
                one(json!({"options": [
                    {"label": "A", "description": "a"}, {"label": "A", "description": "b"}]})),
                "repeats",
            ),
            (
                one(json!({"options": [
                    {"label": "A", "description": "a", "default": true},
                    {"label": "B", "description": "b", "default": true}]})),
                "default",
            ),
        ];
        for (value, expected) in cases {
            let err = parse(&value).unwrap_err();
            assert!(err.contains(expected), "{value}: {err}");
        }
        let nine: Vec<Value> = (0..9)
            .map(|i| json!({"label": format!("o{i}"), "description": "d"}))
            .collect();
        assert!(
            parse(&one(json!({"options": nine})))
                .unwrap_err()
                .contains("more than 8")
        );
    }

    #[test]
    fn several_defaults_are_fine_in_a_multiple_choice() {
        let value = one(json!({"multiple": true, "options": [
            {"label": "A", "description": "a", "default": true},
            {"label": "B", "description": "b", "default": true}]}));
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn answers_are_checked_against_the_questions() {
        let qs = parse(&one(json!({}))).unwrap();
        let ok = |a: Vec<Vec<&str>>| {
            check_answers(
                &qs,
                a.into_iter()
                    .map(|e| e.into_iter().map(Into::into).collect())
                    .collect(),
            )
        };
        assert_eq!(ok(vec![vec![" A "]]).unwrap(), [["A"]]);
        assert_eq!(
            ok(vec![vec!["my own"]]).unwrap(),
            [["my own"]],
            "custom text"
        );
        assert_eq!(ok(vec![vec![""]]).unwrap(), [Vec::<String>::new()]);
        assert!(ok(vec![vec!["A", "B"]]).unwrap_err().contains("one answer"));
        assert!(ok(vec![]).unwrap_err().contains("Expected 1"));
        let closed = parse(&one(json!({"custom": false}))).unwrap();
        let typed = vec![vec!["my own".to_string()]];
        assert!(
            check_answers(&closed, typed)
                .unwrap_err()
                .contains("listed")
        );
        let multi = parse(&one(json!({"multiple": true}))).unwrap();
        let both = vec![vec!["A".to_string(), "B".to_string()]];
        assert!(check_answers(&multi, both).is_ok());
    }

    #[test]
    fn the_model_gets_the_answers_in_opencodes_wording() {
        let qs = parse(&input(json!([
            {"question": "One?", "header": "1", "options": [{"label": "A", "description": "a"}]},
            {"question": "Two?", "header": "2", "multiple": true,
             "options": [{"label": "X", "description": "x"}, {"label": "Y", "description": "y"}]},
            {"question": "Three?", "header": "3", "options": []},
        ])))
        .unwrap();
        let answers = vec![
            vec!["A".to_string()],
            vec!["X".to_string(), "Y".to_string()],
            vec![],
        ];
        assert_eq!(
            format_answers(&qs, &answers),
            "User has answered your questions: \"One?\"=\"A\", \"Two?\"=\"X, Y\", \"Three?\"=\"Unanswered\". You can now continue with the user's answers in mind."
        );
    }
}
