//! Message writes, discovery and reactions with shared aggregate authorization.

pub mod http;
pub mod queries;
pub mod reactions;
pub(crate) mod reactions_repository;
pub(crate) mod repository;
mod service;

pub use queries::{events_after, events_with_membership, message_snapshot};
pub(crate) use service::enrich_message;
pub use service::{edit_message, send_message, withdraw_message};
