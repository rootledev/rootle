//! Opt-in diagnostic sessions and the common terminal/headless draw boundary.
//! Storage, ordering and limits live in `rootle-trace`; application observations
//! belong to the app and its providers. No trace output goes to the TUI stream.

mod frame;
mod session;

pub use frame::{draw, place_cursor};
pub use session::Session;
