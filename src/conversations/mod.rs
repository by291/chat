//! Direct conversations, durable messages, replay and read receipts.

pub mod events;
pub mod groups;
pub mod http;
pub mod messages;
pub mod model;
pub mod preferences;
pub mod receipts;
pub(crate) mod repository;
pub mod service;

// Existing HTTP/schema consumers retain their public paths after workflow grouping.
pub use groups::http as group_http;
pub use messages::{http as message_http, queries as message_queries, reactions};
