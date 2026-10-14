//! Configuration and external dependency adapters.

pub mod config;
pub mod db;
pub mod mail;
pub(crate) mod mail_config;
pub mod object_storage;
pub mod observability;
pub mod rate_limit;
pub mod web_push;

pub(crate) mod internal_auth;
