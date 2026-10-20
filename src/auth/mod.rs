//! Open registration, authentication and revocable device sessions.

pub mod http;
pub(crate) mod mail_repository;
pub mod mail_worker;
pub mod model;
pub mod password;
pub mod recovery;
pub(crate) mod repository;
pub mod security;
pub(crate) mod security_repository;
pub mod service;
pub(crate) mod validation;
