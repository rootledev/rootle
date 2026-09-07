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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionId(u64);

pub(crate) enum Delivery {
    Partial(Value),
    Response(Value),
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

    pub fn response(&mut self, session: SessionId, id: RequestId, response: Value) -> bool {
        if session != self.session {
            return false;
        }
        self.pending
            .remove(&id)
            .is_some_and(|request| request.sender.send(Delivery::Response(response)).is_ok())
    }

    pub fn partial(&self, session: SessionId, id: RequestId, params: Value) -> bool {
        if session != self.session {
            return false;
        }
        self.pending
            .get(&id)
            .filter(|request| request.streaming)
            .is_some_and(|request| request.sender.send(Delivery::Partial(params)).is_ok())
    }

    pub fn disconnect(&mut self, session: SessionId) {
        if session != self.session {
            return;
        }
        self.pending.clear();
        self.output = OutputState::Closed;
        // Recovery remains owned by its rebuilder until it publishes the
        // handshake outcome. EOF must not admit a second rebuilder.
        if self.lifecycle != Lifecycle::Respawning {
            self.lifecycle = Lifecycle::Dead;
        }
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
