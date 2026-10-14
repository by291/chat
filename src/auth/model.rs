//! Authentication wire DTOs and the private result carrying a newly issued cookie token.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::{types::SessionId, users::model::UserView};

#[derive(Debug, Serialize, sqlx::FromRow, ToSchema)]
pub struct SessionView {
    pub id: SessionId,
    pub device_label: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub is_current: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterInput {
    pub invitation: String,
    pub username: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
    pub device_label: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordChangeInput {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Serialize, ToSchema)]
pub struct EmailStatus {
    pub enabled: bool,
    pub verified_email: Option<String>,
    pub pending_email: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub verification_expires_at: Option<OffsetDateTime>,
}

#[derive(Deserialize, ToSchema)]
pub struct EmailEnrollmentInput {
    pub email: String,
    pub current_password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct EmailVerificationInput {
    pub token: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordRecoveryInput {
    pub email: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordResetInput {
    pub token: String,
    pub new_password: String,
}

pub struct AuthenticatedUser {
    pub user: UserView,
    pub session_id: SessionId,
    pub session_token: String,
    pub expires_at: OffsetDateTime,
}
