//! Export the Rust-owned wire contract for the generated TypeScript client.

use chat_server::{
    error::ApiErrorBody,
    realtime::client::{EphemeralEvent, RealtimeEvent},
    types::{
        ConversationEvent, ConversationEventPayload, ConversationView, DirectInput, EventEnvelope,
        EventPage, LoginInput, MessageSnapshot, MessageView, ReadInput, ReadResult, ReceiptData,
        RegisterInput, SendMessageInput, SessionView, TypingInput, UpdateProfile, UserStatus,
        UserView,
    },
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        chat_server::auth::http::register,
        chat_server::auth::http::login,
        chat_server::auth::http::logout,
        chat_server::auth::http::me,
        chat_server::auth::http::sessions,
        chat_server::auth::http::revoke,
        chat_server::users::http::lookup,
        chat_server::users::http::update,
        chat_server::users::http::status,
        chat_server::conversations::http::direct,
        chat_server::conversations::http::list,
        chat_server::conversations::http::get,
        chat_server::conversations::http::send,
        chat_server::conversations::http::messages,
        chat_server::conversations::http::events,
        chat_server::conversations::http::read,
        chat_server::conversations::http::typing
    ),
    components(schemas(
        UserView,
        SessionView,
        RegisterInput,
        LoginInput,
        UpdateProfile,
        DirectInput,
        ConversationView,
        MessageView,
        SendMessageInput,
        MessageSnapshot,
        ConversationEvent,
        ConversationEventPayload,
        EventEnvelope,
        EventPage,
        ReceiptData,
        ReadInput,
        ReadResult,
        TypingInput,
        UserStatus,
        RealtimeEvent,
        EphemeralEvent,
        ApiErrorBody
    ))
)]
struct ApiDoc;
fn main() -> anyhow::Result<()> {
    // serde_json's default map is BTreeMap: recursively sorted object keys and stable output.
    let document = serde_json::to_value(ApiDoc::openapi())?;
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}
