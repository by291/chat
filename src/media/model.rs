//! Public stable metadata and short-lived transfer grants, without storage identities.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::types::{AttachmentId, ConversationId, MembershipId, UploadAttemptId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentPurpose {
    Message,
    Avatar,
}

impl AttachmentPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Avatar => "avatar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    Uploading,
    Processing,
    Ready,
    Failed,
    Expired,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadInput {
    pub purpose: AttachmentPurpose,
    pub conversation_id: Option<ConversationId>,
    pub membership_id: Option<MembershipId>,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttachmentView {
    pub id: AttachmentId,
    pub purpose: AttachmentPurpose,
    pub state: AttachmentState,
    pub filename: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct UploadGrant {
    pub attachment: AttachmentView,
    pub attempt_id: UploadAttemptId,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
pub struct CompleteInput {
    pub attempt_id: UploadAttemptId,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DownloadVariant {
    Original,
    Thumbnail,
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    #[serde(default = "original")]
    pub variant: DownloadVariant,
}

fn original() -> DownloadVariant {
    DownloadVariant::Original
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct DownloadGrant {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}
