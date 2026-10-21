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

/// The only update kinds we subscribe to are `message` and `poll_answer`
/// (see the `allowed_updates` parameter in serve). Other kinds are never
/// requested, so they are not modeled.
#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
    pub poll_answer: Option<PollAnswer>,
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
