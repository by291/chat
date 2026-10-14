//! Public background-notification contracts; endpoint and key inputs have no Debug representation.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::types::{ConversationId, PushSubscriptionId};

#[derive(Serialize, ToSchema)]
pub struct PushConfigView {
    pub enabled: bool,
    pub vapid_public_key: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct SubscriptionInput {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Serialize, ToSchema)]
pub struct SubscriptionBinding {
    pub id: PushSubscriptionId,
    pub generation: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct ActivityInput {
    pub visible: bool,
    pub focused: bool,
    pub conversation_id: Option<ConversationId>,
}
