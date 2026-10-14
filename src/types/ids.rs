//! Distinct UUID-v7 identifiers prevent accidental cross-entity arguments.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            sqlx::Type,
            ToSchema,
        )]
        #[sqlx(transparent)]
        #[serde(transparent)]
        #[schema(value_type = Uuid)]
        pub struct $name(pub Uuid);
        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
id_type!(UserId);
id_type!(SessionId);
id_type!(ConversationId);
id_type!(MessageId);
id_type!(EventId);
id_type!(MembershipId);
id_type!(AttachmentId);
id_type!(UploadAttemptId);
id_type!(PushSubscriptionId);
