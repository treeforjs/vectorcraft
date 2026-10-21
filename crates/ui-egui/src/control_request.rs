use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::Value;

const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;

/// The transport and UI compete for one dispatch decision. A successful cancellation proves
/// that the UI will never start this request; an already started command cannot be rolled back.
#[derive(Clone, Default)]
pub struct ControlTicket(Arc<AtomicU8>);

impl ControlTicket {
    pub fn cancel_before_start(&self) -> bool {
        self.0.compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok() || self.0.load(Ordering::Acquire) == CANCELLED
    }

    fn start(&self) -> bool {
        self.0.compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
}

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
    ticket: ControlTicket,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, Receiver<ControlResponse>) {
        let (reply, rx) = channel();
        (Self { method: method.into(), params, reply, ticket: ControlTicket::default() }, rx)
    }

    pub fn cancellation(&self) -> ControlTicket {
        self.ticket.clone()
    }

    /// Claim dispatch immediately before touching application state. Cancelled requests are skipped.
    pub fn start(&self) -> bool {
        self.ticket.start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_prevents_dispatch_and_is_idempotent() {
        let (request, _reply) = ControlRequest::new("edit", Value::Null);
        let ticket = request.cancellation();
        assert!(ticket.cancel_before_start());
        assert!(ticket.cancel_before_start());
        assert!(!request.start());
    }

    #[test]
    fn started_requests_cannot_claim_to_have_been_cancelled() {
        let (request, _reply) = ControlRequest::new("edit", Value::Null);
        assert!(request.start());
        assert!(!request.cancellation().cancel_before_start());
        assert!(!request.start());
    }

    #[test]
    fn concurrent_dispatch_and_cancellation_have_exactly_one_winner() {
        for _ in 0..128 {
            let (request, _reply) = ControlRequest::new("edit", Value::Null);
            let ticket = request.cancellation();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let other = Arc::clone(&barrier);
            let worker = std::thread::spawn(move || {
                other.wait();
                request.start()
            });
            barrier.wait();
            let cancelled = ticket.cancel_before_start();
            assert_ne!(worker.join().unwrap(), cancelled);
        }
    }
}
