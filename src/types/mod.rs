//! Shared identity/sequence primitives and the explicit public DTO compatibility facade.

mod ids;
mod sequence;

pub use ids::{
    AttachmentId, ConversationId, EventId, MembershipId, MessageId, PushSubscriptionId, SessionId,
    UploadAttemptId, UserId,
};

pub use sequence::Seq;

pub use crate::auth::model::{LoginInput, PasswordChangeInput, RegisterInput, SessionView};

pub use crate::conversations::model::{
    ConversationEvent, ConversationEventKind, ConversationEventPayload, ConversationPreferences,
    ConversationPreferencesInput, ConversationView, DirectInput, EditMessageInput, EventEnvelope,
    EventPage, EventQuery, MessageContext, MessagePage, MessageReplyView, MessageSearchQuery,
    MessageSearchResult, MessageSnapshot, MessageTargetQuery, MessageView, Page, PageCursor,
    ReactionKind, ReactionSummary, ReactionUpdateData, ReadInput, ReadResult, ReceiptData,
    SendMessageInput, SetReactionInput, TypingInput,
};

pub use crate::users::model::{UpdateProfile, UserStatus, UserView};

pub use crate::media::model::{
    AttachmentView, CompleteInput, DownloadGrant, UploadGrant, UploadInput,
};

#[derive(Clone, Copy, Debug)]
pub struct Actor {
    pub user_id: UserId,
    pub session_id: SessionId,
}

pub struct Created<T> {
    pub value: T,
    pub is_new: bool,
}
