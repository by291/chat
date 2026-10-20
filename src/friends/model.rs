//! Friendship wire contracts expose public profiles only.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{types::UserId, users::model::UserView};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum FriendRequestStatus {
    Pending,
    Accepted,
    Declined,
    Cancelled,
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FriendRequestAction {
    Accept,
    Decline,
    Cancel,
}

#[derive(Deserialize, ToSchema)]
pub struct FriendRequestInput {
    pub user_id: UserId,
}

#[derive(Deserialize, ToSchema)]
pub struct FriendRequestActionInput {
    pub action: FriendRequestAction,
}

#[derive(Serialize, ToSchema)]
pub struct FriendRequestView {
    pub id: Uuid,
    pub sender: UserView,
    pub recipient: UserView,
    pub status: FriendRequestStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema)]
pub struct FriendshipView {
    pub user: UserView,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema)]
pub struct RelationshipView {
    pub user: UserView,
    pub is_friend: bool,
    pub pending_request: Option<FriendRequestView>,
}

#[derive(Deserialize, IntoParams)]
pub struct FriendsQuery {
    pub q: Option<String>,
    pub cursor: Option<Uuid>,
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RequestDirection {
    Incoming,
    Outgoing,
}

#[derive(Deserialize, IntoParams)]
pub struct RequestsQuery {
    pub direction: RequestDirection,
    pub status: Option<FriendRequestStatus>,
    pub cursor: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct FriendsPage {
    pub items: Vec<FriendshipView>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct FriendRequestsPage {
    pub items: Vec<FriendRequestView>,
    pub next_cursor: Option<Uuid>,
    pub pending_incoming_count: i64,
}
