//! Public user/profile/presence DTOs; password and session secrets never appear here.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{AttachmentId, UserId};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct UserView {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProfile {
    pub display_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AvatarInput {
    pub attachment_id: AttachmentId,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserStatus {
    pub online: bool,
}
