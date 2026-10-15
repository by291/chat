//! Compatibility facade for the existing media persistence and worker contracts.
//!
//! Concrete repositories own SQL and row mapping; processing/retirement workflows
//! own transactions. Internal consumers import those owners directly.

pub use super::attachment_repository::{
    AttachmentRow, ObjectRow, attachment, cancel, lock_attachment, lock_quota,
};
pub use super::processing_lifecycle::{candidate, fail, publish, renew_lease, retry};
pub use super::processing_repository::{LeasedJob, claim, lease_live};
pub use super::reference_repository::{
    avatar_pointer, current_avatar, insert_message_reference, message_views, published_object,
    referenced, swap_avatar, visible_message,
};
pub use super::retirement::{
    confirm_attempt_deleted, confirm_object_deleted, prepare_cleanup, prepare_reconciliation,
};
pub use super::retirement_repository::{
    CleanupPlan, RetiredKey, cleanup_attachments, record_reconciliation_attempt, retired_keys,
};
pub use super::upload_repository::{
    AttemptRow, attempt, insert_attachment, insert_attempt, pending_count, seal,
};
