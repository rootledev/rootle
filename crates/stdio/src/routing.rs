//! Production routing state, also exercised by the protocol conformance model.

use parking_lot::{Condvar, Mutex};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::Value;
use std::{collections::HashMap, num::NonZeroU64, sync::mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RequestId(NonZeroU64);
impl RequestId {
    pub fn from_wire(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }
    pub fn wire(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SessionId(u64);
impl SessionId {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

pub(crate) enum Delivery {
    Partial(Value),
    Response(Value),
}

/// What happened to an inbound frame, for diagnostics: delivered to
/// its caller, or the precise reason it was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteOutcome {
    Delivered,
    /// From a reader generation that was already replaced.
    StaleSession,
    /// Allocated once (id ≤ next_id) but no longer pending — timed
    /// out or already answered; ids are never reused.
    RetiredId,
    /// Never allocated by this transport (id > next_id).
    UnknownId,
    /// The slot exists but did not opt into streaming.
    NotStreaming,
    /// The waiting caller is gone (its channel closed).
    ReceiverGone,
}

impl RouteOutcome {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RouteOutcome::Delivered => "delivered",
            RouteOutcome::StaleSession => "stale_session",
            RouteOutcome::RetiredId => "retired_id",
            RouteOutcome::UnknownId => "unknown_id",
            RouteOutcome::NotStreaming => "not_streaming",
            RouteOutcome::ReceiverGone => "receiver_gone",
        }
    }
}
struct PendingRequest {
    sender: mpsc::Sender<Delivery>,
    streaming: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    #[default]
    Alive,
    Respawning,
    Dead,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum OutputState {
    Awaiting,
    #[default]
    Open,
    Closed,
}

#[derive(Default)]
pub(crate) struct Routing {
    next_id: u64,
    pending: HashMap<RequestId, PendingRequest>,
    pub session: SessionId,
    pub lifecycle: Lifecycle,
    pub output: OutputState,
    pub restarts: u32,
    pub restart_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct Shared {
    pub routing: Mutex<Routing>,
    pub changed: Condvar,
}

impl Routing {
    pub fn register(
        &mut self,
        gated: bool,
        streaming: bool,
    ) -> ProviderResult<(RequestId, mpsc::Receiver<Delivery>)> {
        if (gated && self.lifecycle != Lifecycle::Alive) || self.output != OutputState::Open {
            return Err(ProviderError::new(
                ErrorKind::Provider,
                "provider restarting — try again",
            ));
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| ProviderError::other("provider request IDs exhausted"))?;
        let id = RequestId::from_wire(self.next_id).expect("allocated IDs are nonzero");
        let (sender, receiver) = mpsc::channel();
        self.pending
            .insert(id, PendingRequest { sender, streaming });
        Ok((id, receiver))
    }

    pub fn expire(&mut self, id: RequestId) {
        self.pending.remove(&id);
    }

    pub fn response(&mut self, session: SessionId, id: RequestId, response: Value) -> RouteOutcome {
        if session != self.session {
            return RouteOutcome::StaleSession;
        }
        match self.pending.remove(&id) {
            Some(request) => {
                if request.sender.send(Delivery::Response(response)).is_ok() {
                    RouteOutcome::Delivered
                } else {
                    RouteOutcome::ReceiverGone
                }
            }
            None => self.retirement_of(id),
        }
    }

    pub fn partial(&self, session: SessionId, id: RequestId, params: Value) -> RouteOutcome {
        if session != self.session {
            return RouteOutcome::StaleSession;
        }
        match self.pending.get(&id) {
            Some(request) if !request.streaming => RouteOutcome::NotStreaming,
            Some(request) => {
                if request.sender.send(Delivery::Partial(params)).is_ok() {
                    RouteOutcome::Delivered
                } else {
                    RouteOutcome::ReceiverGone
                }
            }
            None => self.retirement_of(id),
        }
    }

    /// An id that has no slot was either allocated and since retired
    /// (timeout, already answered — ids are monotonic and never
    /// reused) or was never ours at all.
    fn retirement_of(&self, id: RequestId) -> RouteOutcome {
        if id.wire() <= self.next_id {
            RouteOutcome::RetiredId
        } else {
            RouteOutcome::UnknownId
        }
    }

    /// Returns how many pending slots were dropped by this disconnect.
    pub fn disconnect(&mut self, session: SessionId) -> usize {
        if session != self.session {
            return 0;
        }
        let dropped = self.pending.len();
        self.pending.clear();
        self.output = OutputState::Closed;
        // Recovery remains owned by its rebuilder until it publishes the
        // handshake outcome. EOF must not admit a second rebuilder.
        if self.lifecycle != Lifecycle::Respawning {
            self.lifecycle = Lifecycle::Dead;
        }
        dropped
    }

    /// Reserve the new reader epoch before stopping the old process. Its
    /// late EOF cannot reset the replacement's handshake/lifecycle state.
    pub fn begin_rebuild(&mut self) {
        self.session.0 = self
            .session
            .0
            .checked_add(1)
            .expect("provider session IDs exhausted");
        self.pending.clear();
        self.lifecycle = Lifecycle::Respawning;
        self.output = OutputState::Awaiting;
        self.restart_error = None;
    }
}

#[cfg(test)]
mod tests;
