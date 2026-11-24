//! StreamElements REST client.
//!
//! Auth uses the channel JWT from the StreamElements dashboard
//! (Account > Channels > Show secrets), sent as `Authorization: Bearer <jwt>`.
//! Endpoint shapes follow the community API reference at
//! <https://github.com/c4ldas/streamelements-api> (api.yaml).

pub mod astro;

use crate::activity::Activity;
use crate::error::{check, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const API_V2: &str = "https://api.streamelements.com/kappa/v2";
pub const API_V3: &str = "https://api.streamelements.com/kappa/v3";

/// Activity types the v2 activities endpoint accepts in its `types` filter.
pub const FEED_TYPES: &[&str] = &[
    "follow", "subscriber", "cheer", "tip", "redemption", "raid", "host", "merch",
];

/// The StreamElements channel (account) the token belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

/// Totals from the StreamElements session (`GET /sessions/{channel}`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTotals {
    pub follower_total: Option<u64>,
    pub follower_session: Option<u64>,
    pub subscriber_total: Option<u64>,
    pub subscriber_session: Option<u64>,
}

/// Alert queue controls accepted by `PUT /kappa/v3/overlays/{channel}/action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertAction {
    Mute,
    Unmute,
    Pause,
    Unpause,
    Skip,
}

impl AlertAction {
    pub fn as_str(self) -> &'static str {
        match self {
            AlertAction::Mute => "mute",
            AlertAction::Unmute => "unmute",
            AlertAction::Pause => "pause",
            AlertAction::Unpause => "unpause",
            AlertAction::Skip => "skip",
        }
    }
}

#[derive(Clone)]
pub struct SeClient {
    http: reqwest::Client,
    jwt: String,
    v2: String,
    v3: String,
}

impl std::fmt::Debug for SeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeClient").field("jwt", &"<redacted>").finish()
    }
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn count(data: &Value, key: &str) -> Option<u64> {
    data.get(key)?.get("count")?.as_u64()
}

impl SeClient {
    pub fn new(jwt: impl Into<String>) -> Result<Self> {
        Self::with_bases(jwt, API_V2, API_V3)
    }

    /// Like `new`, with custom API roots (used by tests).
    pub fn with_bases(jwt: impl Into<String>, v2: &str, v3: &str) -> Result<Self> {
        let jwt = jwt.into().trim().to_string();
        if jwt.is_empty() {
            return Err(Error::Other("the StreamElements token is empty".into()));
        }
        let http = reqwest::Client::builder()
            .user_agent(concat!("Cuepoint/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http, jwt, v2: v2.trim_end_matches('/').into(), v3: v3.trim_end_matches('/').into() })
    }

    pub fn jwt(&self) -> &str {
        &self.jwt
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("Authorization", format!("Bearer {}", self.jwt))
            .header("Accept", "application/json")
    }

    /// `GET /channels/me`: the channel this token belongs to. Also validates the token.
    pub async fn me(&self) -> Result<Channel> {
        let resp = self.auth(self.http.get(format!("{}/channels/me", self.v2))).send().await?;
        Ok(check(resp).await?.json().await?)
    }

    /// `GET /activities/{channel}`: recent feed items, newest first.
    pub async fn activities(&self, channel: &str, limit: u32, days: u32) -> Result<Vec<Activity>> {
        let before = now_ms();
        let after = before.saturating_sub(u128::from(days) * 86_400_000);
        let mut query: Vec<(&str, String)> = vec![
            ("after", after.to_string()),
            ("before", before.to_string()),
            ("limit", limit.clamp(1, 500).to_string()),
            ("origin", "feed".into()),
        ];
        for t in FEED_TYPES {
            query.push(("types", (*t).to_string()));
        }
        let resp = self
            .auth(self.http.get(format!("{}/activities/{channel}", self.v2)).query(&query))
            .send()
            .await?;
        let body: Value = check(resp).await?.json().await?;
        // Without `count=true` the response is a plain array; with it, `{ total, docs }`.
        let docs = match &body {
            Value::Array(a) => a.as_slice(),
            Value::Object(o) => o.get("docs").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]),
            _ => &[],
        };
        let mut out: Vec<Activity> = docs.iter().filter_map(Activity::from_value).collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    /// `GET /sessions/{channel}`: follower and subscriber counters.
    pub async fn session_totals(&self, channel: &str) -> Result<SessionTotals> {
        let resp = self.auth(self.http.get(format!("{}/sessions/{channel}", self.v2))).send().await?;
        let body: Value = check(resp).await?.json().await?;
        let data = body.get("data").unwrap_or(&Value::Null);
        Ok(SessionTotals {
            follower_total: count(data, "follower-total"),
            follower_session: count(data, "follower-session"),
            subscriber_total: count(data, "subscriber-total"),
            subscriber_session: count(data, "subscriber-session"),
        })
    }

    /// `PUT /kappa/v3/overlays/{channel}/action` with `{"action": ...}`.
    /// Mutes, unmutes, pauses, resumes or skips the alert queue.
    pub async fn alert_action(&self, channel: &str, action: AlertAction) -> Result<()> {
        let resp = self
            .auth(self.http.put(format!("{}/overlays/{channel}/action", self.v3)))
            .json(&json!({ "action": action.as_str() }))
            .send()
            .await?;
        check(resp).await?;
        Ok(())
    }

    /// `PUT /overlays/{channel}/reload`: reloads every overlay for the channel.
    pub async fn reload_overlays(&self, channel: &str) -> Result<()> {
        let resp = self
            .auth(self.http.put(format!("{}/overlays/{channel}/reload", self.v2)))
            .json(&json!({}))
            .send()
            .await?;
        check(resp).await?;
        Ok(())
    }

    /// `POST /activities/{channel}/{activityId}/replay`: plays the alert again.
    pub async fn replay(&self, channel: &str, activity_id: &str) -> Result<()> {
        let resp = self
            .auth(self.http.post(format!("{}/activities/{channel}/{activity_id}/replay", self.v2)))
            .json(&json!({}))
            .send()
            .await?;
        check(resp).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_counts_parse() {
        let data = serde_json::json!({
            "follower-total": { "count": 757 },
            "subscriber-total": { "count": 5 },
            "follower-latest": { "name": "x" }
        });
        assert_eq!(count(&data, "follower-total"), Some(757));
        assert_eq!(count(&data, "subscriber-total"), Some(5));
        assert_eq!(count(&data, "follower-latest"), None);
    }

    #[test]
    fn channel_parses_from_me() {
        let c: Channel = serde_json::from_value(serde_json::json!({
            "_id": "5b2e2007760aeb7729487dab", "provider": "twitch", "providerId": "12345678",
            "username": "streamer", "displayName": "Streamer", "avatar": "https://x/y.png",
            "apiToken": "secret-not-kept"
        }))
        .unwrap();
        assert_eq!(c.id, "5b2e2007760aeb7729487dab");
        assert_eq!(c.provider_id.as_deref(), Some("12345678"));
    }

    #[tokio::test]
    async fn sends_documented_requests() {
        use crate::testutil::mock;
        let base = mock(|head, body| {
            let first = head.lines().next().unwrap().to_string();
            assert!(head.contains("authorization: Bearer jwt-1") || head.contains("Authorization: Bearer jwt-1"), "{head}");
            if first.starts_with("GET /v2/activities/ch1?") {
                for t in FEED_TYPES {
                    assert!(first.contains(&format!("types={t}")), "{first}");
                }
                assert!(first.contains("limit=50") && first.contains("origin=feed"));
                assert!(first.contains("after=") && first.contains("before="));
                return (200, r#"[
                    {"_id":"old","type":"follow","createdAt":"2026-11-24T10:00:00.000Z","data":{"username":"a"}},
                    {"_id":"new","type":"cheer","createdAt":"2026-11-24T12:00:00.000Z","data":{"username":"b","amount":100}},
                    {"_id":"bad"}
                ]"#.into());
            }
            if first.starts_with("GET /v2/sessions/ch1 ") {
                return (200, r#"{"data":{"follower-total":{"count":757},"subscriber-total":{"count":5}}}"#.into());
            }
            if first.starts_with("PUT /v3/overlays/ch1/action ") {
                assert_eq!(body, r#"{"action":"skip"}"#);
                return (200, r#"{"key":"alerts.skip","message":"ok"}"#.into());
            }
            if first.starts_with("PUT /v2/overlays/ch1/reload ") {
                return (200, r#"{"ok":true}"#.into());
            }
            if first.starts_with("POST /v2/activities/ch1/act9/replay ") {
                return (200, r#"{"success":true}"#.into());
            }
            if first.starts_with("GET /v2/channels/me ") {
                return (401, r#"{"statusCode":401,"error":"Unauthorized"}"#.into());
            }
            (404, "{}".into())
        })
        .await;
        let c = SeClient::with_bases("jwt-1", &format!("{base}/v2"), &format!("{base}/v3")).unwrap();

        let feed = c.activities("ch1", 50, 30).await.unwrap();
        assert_eq!(feed.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["new", "old"]);

        let s = c.session_totals("ch1").await.unwrap();
        assert_eq!((s.follower_total, s.subscriber_total), (Some(757), Some(5)));

        c.alert_action("ch1", AlertAction::Skip).await.unwrap();
        c.reload_overlays("ch1").await.unwrap();
        c.replay("ch1", "act9").await.unwrap();
        assert!(c.me().await.unwrap_err().is_unauthorized());
    }

    #[test]
    fn empty_token_rejected() {
        assert!(SeClient::new("   ").is_err());
    }
}
