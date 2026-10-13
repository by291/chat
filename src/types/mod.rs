//! Shared identity/sequence primitives and the explicit public DTO compatibility facade.

mod ids;
mod sequence;

pub use ids::{
    AttachmentId, ConversationId, EventId, MembershipId, MessageId, SessionId, UploadAttemptId,
    UserId,
};

pub use sequence::Seq;

pub use crate::auth::model::{LoginInput, RegisterInput, SessionView};

pub use crate::conversations::model::{
    ConversationEvent, ConversationEventKind, ConversationEventPayload, ConversationView,
    DirectInput, EventEnvelope, EventPage, EventQuery, MessagePage, MessageSnapshot, MessageView,
    Page, PageCursor, ReadInput, ReadResult, ReceiptData, SendMessageInput, TypingInput,
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
