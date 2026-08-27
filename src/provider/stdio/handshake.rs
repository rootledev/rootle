//! The initialize handshake (protocol v1, doc/provider-protocol.md):
//! one round trip per process generation — protocol check, provider
//! name, capabilities, and the advisory cache budget (v1.2) travel
//! here. `protocol` must be 1; anything else aborts stdio setup and
//! rootle falls back to github.

use super::StdioProvider;
use crate::provider::{Capabilities, ErrorKind, ProviderError, ProviderResult};
use serde_json::{Value, json};

impl StdioProvider {
    /// The v1 handshake. Also the liveness proof after a rebuild: it
    /// runs ungated so the rebuilder can validate the fresh child
    /// while the transport sits in `Respawning`.
    pub(super) fn handshake(&self) -> ProviderResult<Value> {
        let mut params = json!({ "protocol": 1 });
        if self.cache_bytes > 0 {
            params["cache_bytes"] = json!(self.cache_bytes);
        }
        if let Some(dir) = &self.cache_dir {
            params["cache_dir"] = json!(dir.to_string_lossy());
        }
        let reply = self.exchange("initialize", params, false)?;
        let protocol = reply.get("protocol").and_then(Value::as_u64).unwrap_or(1);
        if protocol != 1 {
            return Err(ProviderError::new(
                ErrorKind::Provider,
                format!("unsupported provider protocol {protocol}"),
            ));
        }
        Ok(reply)
    }

    pub(super) fn initialize_from(mut self, reply: Value) -> ProviderResult<Self> {
        if let Some(name) = reply.get("name").and_then(Value::as_str) {
            self.name = format!("stdio:{name}");
        }
        // v1.3: the provider's modeline icon — a builtin name or a
        // literal glyph; absent means text-only.
        if let Some(icon) = reply.get("icon").and_then(Value::as_str) {
            self.icon = Some(icon.to_string());
        }
        if let Some(caps) = reply.get("capabilities") {
            self.capabilities = Capabilities {
                orgs: caps.get("orgs").and_then(Value::as_bool).unwrap_or(true),
                code_search: caps
                    .get("code_search")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                // v1.3: absent inherits code_search (back-compat).
                file_search: caps
                    .get("file_search")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| {
                        caps.get("code_search")
                            .and_then(Value::as_bool)
                            .unwrap_or(true)
                    }),
            };
        }
        if let Some(bytes) = reply.pointer("/cache/bytes").and_then(Value::as_u64) {
            *self.cache_used.lock() = Some(bytes);
        }
        Ok(self)
    }
}
