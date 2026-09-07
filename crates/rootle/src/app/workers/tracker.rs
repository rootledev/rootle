//! Outstanding-worker tracking for quiescence waits (headless
//! `settle`, plans/0023). Every App spawn counts itself in *before*
//! its thread starts; the ticket it takes counts the worker out when
//! the thread body ends — normal return, error path, or unwind.
//! Nothing reads statuses or channels to guess "done": zero
//! outstanding plus a drained queue IS done.
//!
//! Ordering contract: a worker's last event `send` happens inside the
//! thread body, the ticket's Drop after it. So a zero reading means
//! every finished worker's events are already queued or consumed — a
//! waiter that drains *after* reading zero cannot miss anything.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Shared between `App` (spawn sites) and the drivers that wait on
/// quiescence. Cheap to clone; the count is process-wide per app.
#[derive(Clone, Default)]
pub(crate) struct Outstanding {
    count: Arc<AtomicU64>,
}

impl Outstanding {
    /// Count one worker in, *then* spawn its thread and move the
    /// ticket in. The ticket counts it out on drop — which is also
    /// the spawn-failure path: a `thread::spawn` panic unwinds this
    /// thread and drops the still-unmoved ticket.
    pub(crate) fn track(&self) -> Ticket {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ticket {
            count: self.count.clone(),
        }
    }

    /// Workers currently in flight (spawned, ticket not yet dropped).
    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }
}

/// RAII worker lease: held across the thread body, dropped after the
/// worker's last event send. Panics unwind through it; early returns
/// and error paths all fall to the same drop.
pub(crate) struct Ticket {
    count: Arc<AtomicU64>,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_counts_until_the_ticket_drops() {
        let outstanding = Outstanding::default();
        assert_eq!(outstanding.count(), 0);
        let ticket = outstanding.track();
        assert_eq!(outstanding.count(), 1);
        let second = outstanding.track();
        assert_eq!(outstanding.count(), 2);
        drop(ticket);
        assert_eq!(outstanding.count(), 1);
        drop(second);
        assert_eq!(outstanding.count(), 0);
    }

    #[test]
    fn a_panicking_worker_still_counts_out() {
        // Drop runs on unwind; a worker that panics mid-body must not
        // wedge every future settle at a phantom outstanding count.
        let outstanding = Outstanding::default();
        let tracked = outstanding.clone();
        let handle = std::thread::spawn(move || {
            let _ticket = tracked.track();
            panic!("worker body died");
        });
        assert!(handle.join().is_err(), "the worker did panic");
        assert_eq!(outstanding.count(), 0);
    }
}
