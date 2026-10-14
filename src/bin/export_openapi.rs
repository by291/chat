//! Export the Rust-owned wire contract for the generated TypeScript client.

use chat_server::auth::model::{
    EmailEnrollmentInput, EmailStatus, EmailVerificationInput, PasswordRecoveryInput,
    PasswordResetInput,
};
use chat_server::{
    error::ApiErrorBody,
    media::model::DownloadVariant,
    realtime::client::{EphemeralEvent, RealtimeEvent},
    types::{
        ConversationEvent, ConversationEventPayload, ConversationView, DirectInput, EventEnvelope,
        EventPage, LoginInput, MessageSnapshot, MessageView, PasswordChangeInput, ReadInput,
        ReadResult, ReceiptData, RegisterInput, SendMessageInput, SessionView, TypingInput,
        UpdateProfile, UserStatus, UserView,
    },
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        chat_server::auth::http::register,
        chat_server::auth::http::login,
        chat_server::auth::http::logout,
        chat_server::auth::http::change_password,
        chat_server::auth::http::request_password_recovery,
        chat_server::auth::http::reset_password_from_token,
        chat_server::auth::http::email_status,
        chat_server::auth::http::enroll_email,
        chat_server::auth::http::verify_email,
        chat_server::auth::http::me,
        chat_server::auth::http::sessions,
        chat_server::auth::http::revoke,
        chat_server::users::http::lookup,
        chat_server::users::http::update,
        chat_server::users::http::status,
        chat_server::users::http::set_avatar,
        chat_server::users::http::clear_avatar,
        chat_server::users::http::avatar,
        chat_server::conversations::http::direct,
        chat_server::conversations::http::list,
        chat_server::conversations::http::get,
        chat_server::conversations::http::send,
        chat_server::conversations::http::messages,
        chat_server::conversations::http::events,
        chat_server::conversations::http::read,
        chat_server::conversations::http::typing,
        chat_server::conversations::http::preferences,
        chat_server::conversations::message_http::edit,
        chat_server::conversations::message_http::withdraw,
        chat_server::conversations::message_http::search,
        chat_server::conversations::message_http::context,
        chat_server::conversations::group_http::create,
        chat_server::conversations::group_http::rename,
        chat_server::conversations::group_http::members,
        chat_server::conversations::group_http::role,
        chat_server::conversations::group_http::remove,
        chat_server::conversations::group_http::leave,
        chat_server::conversations::group_http::transfer,
        chat_server::conversations::group_http::invite,
        chat_server::conversations::group_http::revoke,
        chat_server::conversations::group_http::accept,
        chat_server::media::http::create,
        chat_server::media::http::renew,
        chat_server::media::http::complete,
        chat_server::media::http::get,
        chat_server::media::http::cancel,
        chat_server::media::http::download
    ),
    components(schemas(
        UserView,
        SessionView,
        RegisterInput,
        LoginInput,
        PasswordChangeInput,
        EmailStatus,
        EmailEnrollmentInput,
        EmailVerificationInput,
        PasswordRecoveryInput,
        PasswordResetInput,
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
        DownloadVariant,
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
