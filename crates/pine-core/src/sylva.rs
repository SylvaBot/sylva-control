//! Sylva: the streamer's own platform. Cortex signs in with a device code and
//! reads the same activity every Sylva page shows, so the feed is one feed
//! everywhere. API: docs/api.md in haruxee/sylva.

use crate::activity::{Activity, ActivityKind};
use crate::error::{check, Error, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub const BASE: &str = "https://sylva-edge.haruxe.workers.dev";
/// The server's keepalive. Its runtime answers this without waking the channel.
const PING: &str = r#"{"type":"ping"}"#;
const PING_EVERY: Duration = Duration::from_secs(10);
/// A socket that says nothing for this long is dead, even if it never closed.
const QUIET_LIMIT: Duration = Duration::from_secs(25);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Cortex/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Step 1: get a code for the streamer to approve in the browser.
pub async fn start_device_flow(http: &reqwest::Client, base: &str) -> Result<DeviceCode> {
    let resp = http
        .post(format!("{base}/v1/device/code"))
        .json(&json!({ "clientName": "Cortex" }))
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// Step 2: wait for the streamer to answer. Returns the API token.
pub async fn finish_device_flow(
    http: &reqwest::Client,
    base: &str,
    code: &DeviceCode,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut wait = Duration::from_secs(code.interval.max(2));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(code.expires_in.max(60));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Other("sign-in canceled".into())),
            _ = tokio::time::sleep(wait) => {}
        }
        if tokio::time::Instant::now() > deadline {
            return Err(Error::Other("the code expired. Try again.".into()));
        }
        let resp = http
            .post(format!("{base}/v1/device/token"))
            .json(&json!({ "deviceCode": code.device_code }))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return body
                .get("access_token")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| Error::Other("Sylva sent no token".into()));
        }
        match body.get("error").and_then(Value::as_str).unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => wait += Duration::from_secs(5),
            "access_denied" => return Err(Error::Other("you pressed Deny in the browser".into())),
            "expired_token" => return Err(Error::Other("the code expired. Try again.".into())),
            other => {
                return Err(Error::Http { status: status.as_u16(), body: other.to_string() });
            }
        }
    }
}

/// The streamer's channel on Sylva.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SylvaChannel {
    pub key: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Clone)]
pub struct SylvaClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl SylvaClient {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        Ok(Self { http: http_client()?, base: BASE.into(), token: token.into() })
    }

    /// A client for another Sylva, such as a local one under test.
    pub fn with_base(token: impl Into<String>, base: impl Into<String>) -> Result<Self> {
        Ok(Self { http: http_client()?, base: base.into(), token: token.into() })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// The account's own channel (the first one Sylva lists).
    pub async fn channel(&self) -> Result<SylvaChannel> {
        let resp = self.http.get(format!("{}/v1/me", self.base)).bearer_auth(&self.token).send().await?;
        let me: Value = check(resp).await?.json().await?;
        let first = me
            .get("channels")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .cloned()
            .ok_or_else(|| Error::Other("this Sylva account has no channel yet".into()))?;
        Ok(serde_json::from_value(first)?)
    }

    /// Activity, newest first: everything but chat, as every Sylva list shows it.
    /// Returns the events and the newest sequence number the server has.
    pub async fn history(&self, key: &str, limit: u32) -> Result<(Vec<Activity>, u64)> {
        let resp = self
            .http
            .get(format!("{}/v1/channels/{}/history", self.base, enc(key)))
            .query(&[("filter", "alerts"), ("limit", &limit.to_string())])
            .bearer_auth(&self.token)
            .send()
            .await?;
        let body: Value = check(resp).await?.json().await?;
        let head = body.get("headSeq").and_then(Value::as_u64).unwrap_or(0);
        let events = body
            .get("events")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(from_sylva).collect())
            .unwrap_or_default();
        Ok((events, head))
    }

    /// Pause, resume, mute, unmute or skip the alert queue, or reload every
    /// overlay (`reload`). Returns the queue's state.
    pub async fn alerts(&self, key: &str, action: &str) -> Result<AlertState> {
        let resp = self
            .http
            .post(format!("{}/v1/channels/{}/alerts/{action}", self.base, enc(key)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    /// The alert queue now.
    pub async fn alert_state(&self, key: &str) -> Result<AlertState> {
        let resp = self
            .http
            .get(format!("{}/v1/channels/{}/state", self.base, enc(key)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    /// Paid tips waiting for approval before their alert.
    pub async fn held_tips(&self, key: &str) -> Result<Vec<Value>> {
        let resp = self
            .http
            .get(format!("{}/v1/channels/{}/tips/held", self.base, enc(key)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    /// Send a held tip to the stream (`approve`) or refund it (`reject`).
    pub async fn tip_action(&self, key: &str, id: &str, action: &str) -> Result<()> {
        let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let resp = self
            .http
            .post(format!("{}/v1/channels/{}/tips/{id}/{action}", self.base, enc(key)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        check(resp).await?;
        Ok(())
    }

    /// Play an old event's alert next.
    pub async fn replay(&self, key: &str, seq: u64) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/v1/channels/{}/alerts/replay/{seq}", self.base, enc(key)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        check(resp).await?;
        Ok(())
    }
}

/// Channel keys look like `twitch:12826`; the dashboard puts them in paths as they are.
fn enc(key: &str) -> String {
    key.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_')).collect()
}

/// The alert queue, as Sylva reports it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertState {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub queued: u64,
    #[serde(default)]
    pub overlays_connected: u64,
}

/// A Sylva event as Cortex's activity. Chat is left out. The id is the
/// event's sequence number, which is what a replay takes.
pub fn from_sylva(e: &Value) -> Option<Activity> {
    let seq = e.get("seq")?.as_u64()?;
    let text = |k: &str| e.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let kind = match e.get("kind")?.as_str()? {
        "chat" => return None,
        "follow" => ActivityKind::Follow,
        "subscriber" => ActivityKind::Subscriber,
        "giftedSub" => ActivityKind::GiftedSub,
        "communityGift" => ActivityKind::CommunityGift,
        "cheer" => ActivityKind::Cheer,
        "tip" => ActivityKind::Tip,
        "redemption" => ActivityKind::Redemption,
        "raid" => ActivityKind::Raid,
        _ => ActivityKind::Other,
    };
    let actor = e.get("actor");
    let actor_text = |k: &str| actor.and_then(|a| a.get(k)).and_then(Value::as_str).map(str::to_string);
    Some(Activity {
        id: seq.to_string(),
        kind,
        raw_type: text("rawType").unwrap_or_default(),
        provider: text("platform"),
        created_at: text("createdAt"),
        username: actor_text("login"),
        display_name: actor_text("displayName"),
        avatar: None,
        amount: e.get("amount").and_then(Value::as_f64),
        currency: text("currency"),
        message: text("message"),
        tier: text("tier"),
        gifted: e.get("gifted").and_then(Value::as_bool).unwrap_or(false),
        sender: text("sender"),
        redemption: text("redemption"),
        raw: e.get("raw").filter(|r| r.is_object()).cloned(),
        test: e.get("platform").and_then(Value::as_str) == Some("sylva"),
        received_at: e.get("receivedAt").and_then(Value::as_i64),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LiveStatus {
    Connecting,
    Live,
    Reconnecting { reason: String },
    /// Sylva no longer accepts the token (the app was removed). Sign in again.
    Unauthorized,
}

#[derive(Debug, Clone)]
pub enum LiveEvent {
    Status(LiveStatus),
    Activity(Box<Activity>),
    /// How many tips wait for approval changed.
    Tips(u32),
}

/// Live activity until `cancel` fires. Reconnects on its own and asks for
/// everything after the last event it saw, so nothing is missed in between.
pub async fn run_live(
    client: SylvaClient,
    key: String,
    mut last_seq: u64,
    tx: mpsc::Sender<LiveEvent>,
    cancel: CancellationToken,
) {
    let mut backoff = Duration::from_millis(500);
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let _ = tx.send(LiveEvent::Status(LiveStatus::Connecting)).await;
        let result = tokio::select! {
            _ = cancel.cancelled() => return,
            r = session(&client, &key, &mut last_seq, &tx, &mut backoff) => r,
        };
        match result {
            Err(Error::Unauthorized { .. }) => {
                let _ = tx.send(LiveEvent::Status(LiveStatus::Unauthorized)).await;
                return;
            }
            Err(e) => {
                let _ = tx.send(LiveEvent::Status(LiveStatus::Reconnecting { reason: e.to_string() })).await;
            }
            Ok(()) => {
                let _ = tx
                    .send(LiveEvent::Status(LiveStatus::Reconnecting { reason: "connection closed".into() }))
                    .await;
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(
    client: &SylvaClient,
    key: &str,
    last_seq: &mut u64,
    tx: &mpsc::Sender<LiveEvent>,
    backoff: &mut Duration,
) -> Result<()> {
    let url = format!("{}/v1/channels/{}/ws", client.base.replacen("http", "ws", 1), enc(key));
    let mut req = url.into_client_request().map_err(|e| Error::Other(e.to_string()))?;
    let auth = format!("Bearer {}", client.token)
        .parse()
        .map_err(|_| Error::Other("bad token".into()))?;
    req.headers_mut().insert("Authorization", auth);
    let (ws, _) = match tokio_tungstenite::connect_async(req).await {
        Ok(x) => x,
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => {
            let status = r.status().as_u16();
            return Err(if status == 401 || status == 403 {
                Error::Unauthorized { status, body: String::new() }
            } else {
                Error::Http { status, body: String::new() }
            });
        }
        Err(e) => return Err(Error::Other(e.to_string())),
    };
    let (mut sink, mut stream) = ws.split();
    let hello = json!({ "type": "hello", "role": "feed", "lastSeq": *last_seq }).to_string();
    sink.send(Message::Text(hello)).await.map_err(|e| Error::Other(e.to_string()))?;

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if heard.elapsed() > QUIET_LIMIT {
                    return Err(Error::Other("Sylva went quiet".into()));
                }
                sink.send(Message::Text(PING.into())).await.map_err(|e| Error::Other(e.to_string()))?;
            }
            msg = stream.next() => {
                let Some(msg) = msg else { return Ok(()) };
                let msg = msg.map_err(|e| Error::Other(e.to_string()))?;
                heard = tokio::time::Instant::now();
                let Message::Text(text) = msg else {
                    if let Message::Close(_) = msg { return Ok(()); }
                    continue;
                };
                let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
                match frame.get("type").and_then(Value::as_str) {
                    Some("ready") => {
                        *backoff = Duration::from_millis(500);
                        let _ = tx.send(LiveEvent::Status(LiveStatus::Live)).await;
                    }
                    Some("tips") => {
                        let held = frame.get("held").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let _ = tx.send(LiveEvent::Tips(held)).await;
                    }
                    Some("event") => {
                        if let Some(seq) = frame.get("seq").and_then(Value::as_u64) {
                            *last_seq = (*last_seq).max(seq);
                        }
                        if let Some(a) = from_sylva(&frame) {
                            let _ = tx.send(LiveEvent::Activity(Box::new(a))).await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_events_and_skips_chat() {
        let a = from_sylva(&json!({
            "type": "event", "seq": 42, "receivedAt": 1, "id": "x", "channel": "twitch:1", "platform": "twitch",
            "kind": "cheer", "rawType": "channel.cheer", "createdAt": "2026-11-26T00:00:00Z",
            "actor": { "id": "1", "login": "maple", "displayName": "Maple" }, "amount": 100.0, "gifted": false,
            "raw": { "bits": 100 }
        }))
        .unwrap();
        assert_eq!(a.id, "42");
        assert_eq!(a.kind, ActivityKind::Cheer);
        assert_eq!(a.display_name.as_deref(), Some("Maple"));
        assert!(!a.test);
        assert!(from_sylva(&json!({ "seq": 1, "kind": "chat" })).is_none());
        let t = from_sylva(&json!({ "seq": 2, "kind": "follow", "platform": "sylva" })).unwrap();
        assert!(t.test);
    }
}
