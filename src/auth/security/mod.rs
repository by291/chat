//! Credential changes, verified mailbox enrollment and account recovery.

pub mod recovery;
pub(crate) mod repository;
mod service;

pub use crate::auth::mail::enqueue_mail;
pub use service::{
    change_password, email_status, enroll_email, invalidate_security_challenges, verify_email,
};
pub(crate) use service::{
    invalid_token, load_token, normalize_email, require_mail, validate_challenge,
};
