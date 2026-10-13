//! Private realtime authorization, bounded transport, and durable outbox delivery.

pub mod client;
pub mod delivery;
pub mod http;
pub mod model;
pub mod outbox;
pub(crate) mod repository;
pub mod service;
