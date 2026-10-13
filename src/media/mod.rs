//! Scoped upload grants, immutable processing, references and retryable cleanup.

pub mod http;
pub mod model;
pub mod processing;
pub mod repository;
pub mod service;
pub mod worker;

pub const MAX_UPLOAD_BYTES: u64 = 52_428_800;
pub const MAX_PENDING_UPLOADS: i64 = 10;
pub const MAX_MESSAGE_ATTACHMENTS: usize = 10;
pub const UPLOAD_GRANT_SECONDS: i64 = 600;
// A PUT accepted just before grant expiry can finish later. Keep staging bytes
// reserved through public ingress's 600s body / 630s write ceilings and drain buffer.
// Operators must keep the ingress deadlines within this 660s cleanup grace.
pub const UPLOAD_CLEANUP_GRACE_SECONDS: i64 = 660;
pub const DOWNLOAD_GRANT_SECONDS: i64 = 60;
