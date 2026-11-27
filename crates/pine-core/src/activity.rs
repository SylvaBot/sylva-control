//! The activity model the window shows, for Sylva's history and its live socket.

use serde::Serialize;
use serde_json::Value;

/// The feed categories the UI filters on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Follow,
    Subscriber,
    GiftedSub,
    CommunityGift,
    Cheer,
    Tip,
    Redemption,
    Raid,
    Host,
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub kind: ActivityKind,
    /// The platform's event type, such as `channel.follow`.
    pub raw_type: String,
    pub provider: Option<String>,
    pub created_at: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub message: Option<String>,
    pub tier: Option<String>,
    pub gifted: bool,
    pub sender: Option<String>,
    pub redemption: Option<String>,
    /// The platform's own payload, for events Sylva Desktop words from its fields
    /// (ad breaks, polls, predictions). Only Sylva sends it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    /// A test event from a "Try it" button. It leaves the feed after 30 seconds.
    pub test: bool,
    /// When the server got it, in Unix milliseconds. Only Sylva sends it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_at: Option<i64>,
}
