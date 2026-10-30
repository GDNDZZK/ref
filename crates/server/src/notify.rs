//! System notifications: which connection, if any, gets a `ServerFrame::Notify`.
//!
//! The user may have several windows open (GUI, browser tabs). A notification is for a user who
//! is not looking at the session, and it goes to one window only:
//!
//! 1. Some connection is in front (`SetFocus`) and shows the session: nothing is sent.
//! 2. Otherwise the connections that asked for this kind (`Hello.notify`, `SetNotify`) compete.
//!    The one that came to the front most recently wins; a connection that never did counts
//!    from its connection time.

use crate::hub::Hub;
use crate::session::SessionState;
use ref_proto::{NotifyKind, ServerFrame};

impl Hub {
    /// Notifies the user about `st`'s session, following the rules in the module docs.
    /// `detail` is the tool name (permission) or the error text (turn_error).
    pub fn notify(&self, st: &SessionState, kind: NotifyKind, detail: Option<String>) {
        let session_id = &st.info.id;
        let mut winner = None;
        let mut winner_stamp = 0;
        for conn in self.conns() {
            let c = conn.lock();
            if !c.hello {
                continue;
            }
            if c.focused && c.shown.contains(session_id) {
                return;
            }
            if !c.notify.contains(&kind) {
                continue;
            }
            if winner.is_none() || c.focus_stamp > winner_stamp {
                winner_stamp = c.focus_stamp;
                drop(c);
                winner = Some(conn);
            }
        }
        let Some(conn) = winner else { return };
        // Sessions that have no title yet (the first turn) are shown by name.
        let title = if st.info.title.is_empty() {
            &st.info.name
        } else {
            &st.info.title
        };
        conn.send(ServerFrame::Notify {
            session_id: session_id.clone(),
            notify_kind: kind,
            session_title: title.clone(),
            detail,
        });
    }
}
