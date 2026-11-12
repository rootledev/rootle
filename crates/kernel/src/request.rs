//! A request accepts partial replies until exactly one terminal reply lands.

#[derive(Clone, Copy)]
pub enum Phase {
    Idle,
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Copy)]
pub enum Reply {
    Partial,
    Complete,
    Failed,
}

/// Decide acceptance and the next phase in one operation. Rejection leaves
/// the caller's state untouched. Payload identity is additionally checked by
/// the adapter; generation equality alone must never override that check.
pub fn accept(current: u64, phase: Phase, incoming: u64, reply: Reply) -> Option<Phase> {
    if current != incoming {
        return None;
    }
    match phase {
        Phase::Loading => Some(match reply {
            Reply::Partial => Phase::Loading,
            Reply::Complete => Phase::Ready,
            Reply::Failed => Phase::Failed,
        }),
        Phase::Idle | Phase::Ready | Phase::Failed => None,
    }
}
