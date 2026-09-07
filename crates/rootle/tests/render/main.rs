//! Frame-level verification with ratatui's TestBackend (see
//! .agents/skills/rootle-tui-debug): renders the app to a Buffer and
//! asserts on visible text — including that closing a popup leaves
//! no lingering cells.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use rootle::app::App;

mod fixtures;
use fixtures::*;

mod commits;
mod lifecycle;
mod loading;
mod navigation;
mod overlays;
mod preview;
mod revisions;
mod search_files;
mod search_queries;
mod search_streaming;
