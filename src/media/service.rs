//! Compatibility facade for the public media application operations.
//!
//! Internal consumers depend on upload/reference workflow owners directly; existing
//! callers retain these names while transaction ownership stays with those workflows.

pub use super::references::{avatar_id, bind_avatar, bind_message, download, get_attachment};
pub use super::uploads::{
    cancel_upload, complete_upload, create_upload, renew_upload, validate_upload,
};
