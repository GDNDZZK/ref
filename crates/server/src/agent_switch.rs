//! Tells the model when the user switches the agent mid-conversation, so earlier messages
//! (written under another agent's instructions) do not mislead it.
//!
//! The note is a `Reminder` user message, added without a model turn. Quick switches (Tab
//! pressed several times) share one message that always says "from the agent the model last
//! saw", and switching back removes it.
//!
//! While a run is active the message is not written at once: the model may be reading the
//! history. The switch is only remembered and written by [`deliver`] under the lock that
//! reads the agent for the next request, so the reminder and the new agent take effect
//! together. The memory is not persisted: a reload or a backend restart forgets a switch that
//! was not delivered yet.

use crate::hub::Hub;
use crate::session::{Session, SessionState, user_message};
use ref_proto::{Message, MessageSource, Part, Role, SessionEvent, now_ms};
use std::sync::Arc;

/// The last switch the model has not answered yet.
pub struct AgentSwitch {
    /// The agent the model last saw.
    from: String,
    /// The reminder message, once it is part of the conversation.
    message_id: Option<String>,
}

fn reminder_text(from: &str, to: &str) -> String {
    format!(
        "[Reminder] The user switched the agent from {from} to {to}. Your instructions and available tools now come from the {to} agent; where earlier messages conflict with them, follow the {to} agent."
    )
}

/// The remembered switch if it can still be updated: not delivered, or delivered and not yet
/// answered by the model. A delivered one is final while a run is active.
fn take_open(st: &mut SessionState) -> Option<AgentSwitch> {
    let note = st.agent_switch.take()?;
    let Some(id) = &note.message_id else {
        return Some(note);
    };
    let at = st.find_message(id)?;
    let busy = st.run.is_some();
    let answered = st.messages[at + 1..]
        .iter()
        .any(|m| m.role == Role::Assistant);
    (!busy && !answered).then_some(note)
}

fn emit_message(hub: &Arc<Hub>, session_id: &str, message: &Message) {
    let message = message.clone();
    hub.emit(session_id, SessionEvent::Message { message });
}

/// Call after the session's agent changed from `old` to `st.info.agent`.
pub fn switched(hub: &Arc<Hub>, s: &Arc<Session>, old: &str) {
    {
        let mut st = s.lock();
        let note = take_open(&mut st);
        let seen = note.as_ref().map_or(old, |n| n.from.as_str()).to_string();
        let current = st.info.agent.clone();
        let reminder_id = note.and_then(|n| n.message_id);
        if seen == current {
            // Back to the agent the model knows: nothing to tell.
            if let Some(id) = reminder_id {
                remove_reminder(hub, &mut st, &id);
            }
        } else if st.run.is_some() {
            st.agent_switch = Some(AgentSwitch {
                from: seen,
                message_id: None,
            });
        } else {
            let id = write_reminder(hub, &mut st, reminder_id, &seen);
            st.agent_switch = Some(AgentSwitch {
                from: seen,
                message_id: Some(id),
            });
        }
    }
    crate::ops::persist(hub, s);
}

/// Writes a switch made during a run into the conversation. Call where the agent for the next
/// request is read, with the same lock held.
pub fn deliver(hub: &Arc<Hub>, st: &mut SessionState) {
    let Some(note) = &st.agent_switch else { return };
    if note.message_id.is_some() {
        return;
    }
    let from = note.from.clone();
    let id = write_reminder(hub, st, None, &from);
    st.agent_switch = Some(AgentSwitch {
        from,
        message_id: Some(id),
    });
}

/// Adds the reminder, or rewrites the one with id `existing`. Returns its id.
fn write_reminder(
    hub: &Arc<Hub>,
    st: &mut SessionState,
    existing: Option<String>,
    from: &str,
) -> String {
    let text = reminder_text(from, &st.info.agent);
    let at = existing.as_deref().and_then(|id| st.find_message(id));
    if let Some(at) = at {
        let message = &mut st.messages[at];
        message.parts = vec![Part::Text {
            text,
            time: now_ms(),
        }];
        emit_message(hub, &st.info.id, message);
        return message.id.clone();
    }
    let message = user_message(MessageSource::Reminder, text, vec![]);
    let id = message.id.clone();
    emit_message(hub, &st.info.id, &message);
    st.push_message(message);
    hub.emit_info(st);
    id
}

fn remove_reminder(hub: &Arc<Hub>, st: &mut SessionState, id: &str) {
    let Some(at) = st.find_message(id) else {
        return;
    };
    st.messages.remove(at);
    let message_id = id.to_string();
    hub.emit(&st.info.id, SessionEvent::Removed { message_id });
    hub.emit_info(st);
}
