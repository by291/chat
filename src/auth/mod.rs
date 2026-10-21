//! Open registration, authentication and revocable device sessions.

pub mod http;
pub mod mail;
pub mod model;
pub mod password;
pub(crate) mod repository;
pub mod security;
pub mod service;
pub(crate) mod validation;

// Public compatibility paths remain stable for existing worker and recovery consumers.
pub use mail::worker as mail_worker;
pub use security::recovery;
