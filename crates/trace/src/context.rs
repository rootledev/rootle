//! Thread identity and operation correlation stamped into every envelope.
//!
//! [`operation_id`] allocates a correlation identity only while a trace is
//! enabled, so job-spawn sites pay nothing when capture is off.
//! [`in_operation`] installs that identity in a thread-local for the
//! duration of the work — including through provider/HTTP calls that record
//! on their own — and restores the previous identity even when the work
//! panics.

use serde::{Deserialize, Serialize};
use std::cell::{Cell, OnceCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::enabled;

/// Correlation identity linking a job's lifecycle records across threads.
/// Serializes as its bare number inside envelopes and producer payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub u64);

static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);
static THREAD_SERIAL: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static OPERATION: Cell<Option<OperationId>> = const { Cell::new(None) };
    static THREAD_LABEL: OnceCell<Arc<str>> = const { OnceCell::new() };
}

/// Allocate a fresh correlation identity, but only while a trace is
/// enabled. Returns `None` (and allocates nothing) for disabled capture;
/// pass the `Option` straight through to [`in_operation`].
pub fn operation_id() -> Option<OperationId> {
    enabled().then(|| OperationId(NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)))
}

/// The identity installed on this thread, stamped into records admitted
/// here. Absent outside [`in_operation`] — including on the spawning
/// thread that only allocated the identity.
pub(crate) fn current_operation() -> Option<OperationId> {
    OPERATION.get()
}

/// Run `work` with `id` as this thread's correlation identity, restoring
/// whatever was installed before — including on panic unwind, so a
/// panicking worker cannot leak its identity into unrelated records.
pub fn in_operation<T>(id: Option<OperationId>, work: impl FnOnce() -> T) -> T {
    let _restore = OperationGuard::enter(id);
    work()
}

struct OperationGuard {
    previous: Option<OperationId>,
}

impl OperationGuard {
    fn enter(id: Option<OperationId>) -> Self {
        Self {
            previous: OPERATION.replace(id),
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        OPERATION.set(self.previous);
    }
}

/// A stable per-thread label for the envelope's `thread` field: the
/// platform thread name when one exists, disambiguated by a process-wide
/// serial so unnamed workers still stay distinguishable.
pub(crate) fn thread_label() -> Arc<str> {
    THREAD_LABEL.with(|cell| {
        Arc::clone(cell.get_or_init(|| {
            let current = std::thread::current();
            let serial = THREAD_SERIAL.fetch_add(1, Ordering::Relaxed);
            format!("{}-{serial}", current.name().unwrap_or("unnamed")).into()
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_restores_through_nested_work() {
        let outer = OperationId(7);
        let _restore = OperationGuard::enter(Some(outer));
        assert_eq!(current_operation(), Some(outer));
        in_operation(None, || assert_eq!(current_operation(), None));
        assert_eq!(current_operation(), Some(outer));
        let inner = OperationId(9);
        let seen = in_operation(Some(inner), current_operation);
        assert_eq!(seen, Some(inner));
        assert_eq!(current_operation(), Some(outer));
    }

    #[test]
    fn thread_labels_are_stable_and_distinct() {
        let first = thread_label();
        assert!(
            Arc::ptr_eq(&first, &thread_label()),
            "label computed once per thread"
        );
        let other = std::thread::Builder::new()
            .name("probe".into())
            .spawn(thread_label)
            .unwrap()
            .join()
            .unwrap();
        assert!(first != other, "distinct threads get distinct labels");
        assert!(
            other.starts_with("probe-"),
            "label carries the name: {other}"
        );
    }
}
