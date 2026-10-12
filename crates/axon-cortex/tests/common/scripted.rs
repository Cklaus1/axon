//! A scripted OpenRouter transport for the ranking and audit tests: it records
//! every request and answers each from a list, in order. Nothing reaches the
//! network.

use axon_cortex::openrouter::Transport;
use serde_json::Value;
use std::cell::RefCell;
use std::time::Duration;

/// A key shaped like a real one, so a leak of it into a reason is findable.
pub const KEY: &str = "sk-or-test-KEY-0123456789";

/// Records every request and answers each from `replies`, in order.
pub struct Scripted {
    replies: RefCell<Vec<Result<(u16, String), String>>>,
    seen: RefCell<Vec<(String, String, Value)>>,
}

impl Scripted {
    pub fn new(replies: Vec<Result<(u16, String), String>>) -> Self {
        Scripted {
            replies: RefCell::new(replies),
            seen: RefCell::new(Vec::new()),
        }
    }
    /// `(url, key, body)` per request, in order.
    pub fn requests(&self) -> Vec<(String, String, Value)> {
        self.seen.borrow().clone()
    }
}

impl Transport for Scripted {
    fn post_json(
        &self,
        url: &str,
        key: &str,
        body: &Value,
        _timeout: Duration,
    ) -> Result<(u16, String), String> {
        self.seen
            .borrow_mut()
            .push((url.to_string(), key.to_string(), body.clone()));
        let mut r = self.replies.borrow_mut();
        assert!(!r.is_empty(), "more requests than scripted replies");
        r.remove(0)
    }
}
