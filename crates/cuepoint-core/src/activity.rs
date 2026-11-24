//! One normalized activity model for both the REST history and the live websocket.
//!
//! StreamElements sends the same document shape from `GET /kappa/v2/activities/{channel}`
//! and from the Astro `channel.activities` topic:
//! `{ _id, type, provider, channel, createdAt, data: { username, displayName, avatar, amount, ... } }`.
//! Fields inside `data` vary by type, so everything except the id and type is optional.

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
    /// The `type` string exactly as StreamElements sent it.
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
}

fn s(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(x) if !x.is_empty() => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn num(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(x) => x.parse().ok(),
        _ => None,
    }
}

fn truthy(v: &Value, key: &str) -> bool {
    matches!(v.get(key), Some(Value::Bool(true)))
}

impl Activity {
    /// Parse one activity document. Returns `None` if it has no id or type.
    pub fn from_value(doc: &Value) -> Option<Activity> {
        let id = s(doc, "_id").or_else(|| s(doc, "activityId"))?;
        let raw_type = s(doc, "type")?;
        let empty = Value::Null;
        let data = doc.get("data").unwrap_or(&empty);

        let gifted = truthy(data, "gifted") || truthy(data, "bulkGifted");
        let sender = s(data, "sender");

        let kind = match raw_type.as_str() {
            "follow" | "follower" => ActivityKind::Follow,
            "subscriber" if gifted || sender.is_some() => ActivityKind::GiftedSub,
            "subscriber" => ActivityKind::Subscriber,
            "communityGiftPurchase" => ActivityKind::CommunityGift,
            "cheer" => ActivityKind::Cheer,
            "tip" => ActivityKind::Tip,
            "redemption" | "channelPointsRedemption" => ActivityKind::Redemption,
            "raid" => ActivityKind::Raid,
            "host" => ActivityKind::Host,
            _ => ActivityKind::Other,
        };

        Some(Activity {
            id,
            kind,
            raw_type,
            provider: s(doc, "provider"),
            created_at: s(doc, "createdAt"),
            username: s(data, "username"),
            display_name: s(data, "displayName"),
            avatar: s(data, "avatar"),
            amount: num(data, "amount"),
            currency: s(data, "currency"),
            message: s(data, "message"),
            tier: s(data, "tier"),
            gifted,
            sender,
            redemption: s(data, "redemption"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_follow() {
        let a = Activity::from_value(&json!({
            "_id": "6758d03f3f8de96843d1d35c", "type": "follow", "provider": "twitch",
            "createdAt": "2024-12-10T23:35:27.941Z",
            "data": { "username": "mrsim", "displayName": "MrSim", "providerId": "1159028596",
                      "quantity": 0, "avatar": "https://example.com/a.png" }
        }))
        .unwrap();
        assert_eq!(a.kind, ActivityKind::Follow);
        assert_eq!(a.display_name.as_deref(), Some("MrSim"));
        assert_eq!(a.amount, None);
    }

    #[test]
    fn parses_gift_and_cheer_and_redemption() {
        let gift = Activity::from_value(&json!({
            "_id": "1", "type": "subscriber",
            "data": { "amount": 1, "username": "clairajennifer", "sender": "haruxee", "gifted": true, "tier": "1000" }
        }))
        .unwrap();
        assert_eq!(gift.kind, ActivityKind::GiftedSub);
        assert_eq!(gift.sender.as_deref(), Some("haruxee"));
        assert_eq!(gift.tier.as_deref(), Some("1000"));

        let cheer = Activity::from_value(&json!({
            "_id": "2", "type": "cheer", "data": { "amount": 100, "username": "x", "message": "Cheer100 hi" }
        }))
        .unwrap();
        assert_eq!(cheer.kind, ActivityKind::Cheer);
        assert_eq!(cheer.amount, Some(100.0));

        let red = Activity::from_value(&json!({
            "_id": "3", "type": "redemption", "data": { "amount": 0, "username": "u", "redemption": "10 push ups" }
        }))
        .unwrap();
        assert_eq!(red.kind, ActivityKind::Redemption);
        assert_eq!(red.redemption.as_deref(), Some("10 push ups"));
    }

    #[test]
    fn uses_activity_id_when_no_underscore_id() {
        let a = Activity::from_value(&json!({ "activityId": "abc", "type": "raid", "data": {} })).unwrap();
        assert_eq!(a.id, "abc");
        assert_eq!(a.kind, ActivityKind::Raid);
    }

    #[test]
    fn rejects_documents_without_type() {
        assert!(Activity::from_value(&json!({ "_id": "x" })).is_none());
    }
}
