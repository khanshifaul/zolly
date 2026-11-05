//! Telegram Bot API wire types. Deserialize-only mirrors of the JSON
//! shapes we consume; fields we never read are kept for documentation and
//! marked dead_code at the struct level.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TelegramResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub description: Option<String>,
    pub parameters: Option<ResponseParameters>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseParameters {
    pub retry_after: Option<u64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    pub username: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub from: Option<User>,
    pub chat: Chat,
    pub date: i64,
    pub text: Option<String>,
    pub voice: Option<Voice>,
    /// Set when the founder attached text to a file instead of sending bare text.
    pub caption: Option<String>,
    pub document: Option<Document>,
    pub photo: Option<Vec<PhotoSize>>,
}

/// A file sent to the bot. Telegram delivers it as a `message` with
/// `document` set; `file_id` is the only handle `getFile` needs.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Document {
    pub file_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}

/// One size of a sent photo. Telegram sends several; the handler keeps the
/// largest by dimensions rather than trusting the array order, which the Bot
/// API does not promise.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct PhotoSize {
    pub file_id: String,
    pub width: i64,
    pub height: i64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Voice {
    pub file_id: String,
    pub duration: i64,
}

#[derive(Debug, Deserialize)]
pub struct PollAnswer {
    pub poll_id: String,
    pub user: User,
    pub option_ids: Vec<i32>,
}

/// One reaction on a message: Telegram sends the emoji as a `ReactionType`
/// whose `type` is `emoji`, `custom_emoji` or `paid`.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ReactionType {
    #[serde(rename = "type")]
    pub kind: String,
    pub emoji: Option<String>,
}

/// `message_reaction` — someone added, changed or removed a reaction.
/// `old_reaction` and `new_reaction` are the state before and after, so a
/// removal is an empty `new_reaction` rather than a separate update shape.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct MessageReaction {
    pub chat: Chat,
    pub message_id: i64,
    pub user: Option<User>,
    pub date: i64,
    #[serde(default)]
    pub old_reaction: Vec<ReactionType>,
    #[serde(default)]
    pub new_reaction: Vec<ReactionType>,
}

/// The update kinds we subscribe to are `message`, `voice`, `poll_answer` and
/// `message_reaction` (see the `allowed_updates` parameter in serve and
/// webhook). Other kinds are never requested, so they are not modeled.
#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
    pub poll_answer: Option<PollAnswer>,
    pub message_reaction: Option<MessageReaction>,
}

#[derive(Debug, Deserialize)]
pub struct FileResponse {
    pub ok: bool,
    pub result: Option<FileResult>,
    pub description: Option<String>,
}

/// `getWebhookInfo` result. `url` is empty (not absent) when no webhook is set —
/// the guard treats empty as "none". A full mirror of the wire shape for
/// documentation; only the fields the CLI and the long-poll guard read are used.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct WebhookInfo {
    pub url: Option<String>,
    pub has_custom_certificate: Option<bool>,
    pub pending_update_count: i64,
    pub last_error_date: Option<i64>,
    pub last_error_message: Option<String>,
    pub max_connections: Option<i64>,
    pub allowed_updates: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct FileResult {
    pub file_path: Option<String>,
}
