//! Telegram subsystem: config, wire types, text formatting, outbound
//! delivery, the long-poll listener, and the chat/command handlers.
//!
//! Layout:
//!   config  — token, allowlist, HTTP client
//!   types   — Telegram Bot API wire shapes
//!   format  — pure text helpers (chunking, markdown, keywords)
//!   send    — outbound delivery + voice synthesis/transcription
//!   serve   — getUpdates loop, offset persistence, update dispatch
//!   webhook — the webhook transport: HTTP listener + set/info/delete CLI
//!   handlers— queue-backed keywords, watch, day journal, report, collab

pub mod config;
pub mod format;
pub mod handlers;
pub mod send;
pub mod serve;
pub mod types;
pub mod webhook;

pub use handlers::{cmd_collab, cmd_day, cmd_report, cmd_watch, DayAction};
pub use send::{cmd_check, cmd_poll, cmd_reply, cmd_send, cmd_send_file, cmd_voice};
