//! Counts how often the same request came, so a script can fail its first tries (`retry`).
//!
//! The engine repeats a failed request unchanged, so "the same request" is the same session with
//! the same number of messages. A request body carries no message ids, hence two safeguards
//! keep a finished demonstration from leaking into the next one (after an undo the count
//! repeats): a successful answer forgets the count, and so does a pause longer than the
//! retry backoff.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longer than any pause between two retries of the same request in the mock.
const RETRY_WINDOW: Duration = Duration::from_secs(30);

type Key = (String, usize);

struct Seen {
    count: u32,
    last: Instant,
}

#[derive(Default)]
pub struct Attempts {
    seen: Mutex<HashMap<Key, Seen>>,
}

impl Attempts {
    /// Registers a request and returns how many identical ones came, this one included.
    pub fn count(&self, session: &str, messages: usize) -> u32 {
        self.count_at(session, messages, Instant::now())
    }

    fn count_at(&self, session: &str, messages: usize, now: Instant) -> u32 {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let entry = seen.entry((session.to_string(), messages)).or_insert(Seen {
            count: 0,
            last: now,
        });
        if now.duration_since(entry.last) > RETRY_WINDOW {
            entry.count = 0;
        }
        entry.count += 1;
        entry.last = now;
        entry.count
    }

    /// The request was answered: the next identical one starts a new demonstration.
    pub fn forget(&self, session: &str, messages: usize) {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.remove(&(session.to_string(), messages));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_requests_count_up_until_answered() {
        let attempts = Attempts::default();
        let now = Instant::now();
        assert_eq!(attempts.count_at("s", 3, now), 1);
        assert_eq!(attempts.count_at("s", 3, now), 2);
        assert_eq!(attempts.count_at("s", 5, now), 1, "another request");
        assert_eq!(attempts.count_at("t", 3, now), 1, "another session");
        attempts.forget("s", 3);
        assert_eq!(attempts.count_at("s", 3, now), 1, "after an answer");
    }

    #[test]
    fn a_long_pause_starts_over() {
        let attempts = Attempts::default();
        let now = Instant::now();
        assert_eq!(attempts.count_at("s", 3, now), 1);
        assert_eq!(attempts.count_at("s", 3, now), 2);
        let later = now + RETRY_WINDOW + Duration::from_secs(1);
        assert_eq!(attempts.count_at("s", 3, later), 1);
    }
}
