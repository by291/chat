//! Actor-owned preferences with transaction-composed private invalidation hints.

pub(crate) mod repository;
mod service;

pub use service::update_preferences;
