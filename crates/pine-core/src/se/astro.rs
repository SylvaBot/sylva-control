//! Live events over the StreamElements Astro websocket.
//!
//! Protocol (docs.streamelements.com/websockets):
//! * connect to `wss://astro.streamelements.com`; the server sends `{"type":"welcome", ...}`
//! * send `{"type":"subscribe","nonce":..,"data":{"topic":"channel.activities","room":<channel id>,
//!   "token":<jwt>,"token_type":"jwt"}}`
//! * the server answers `{"type":"response","nonce":..,"data":{...}}`, with an `error` field on failure
//! * events arrive as `{"type":"message","topic":"channel.activities","data":<activity document>}`
//! * keepalive is websocket PING every 30 s; tungstenite answers PONG while we keep reading
//! * on graceful shutdown the server sends `{"type":"reconnect","data":{"reconnect_token":..}}`;
//!   reconnecting with `?reconnect_token=` restores subscriptions without re-subscribing.

use crate::activity::Activity;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub const ASTRO_URL: &str = "wss://astro.streamelements.com";
pub const ACTIVITIES_TOPIC: &str = "channel.activities";
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RealtimeStatus {
    Connecting,
    /// Socket is open; waiting for the subscription to be confirmed.
    Connected,
    /// Subscribed to `channel.activities`; events will arrive.
    Live,
    Reconnecting { in_secs: u64, reason: String },
    /// The server rejected the token. The loop stops; the user must sign in again.
    Unauthorized { message: String },
    Stopped,
}

#[derive(Debug, Clone)]
pub enum AstroEvent {
    Status(RealtimeStatus),
    Activity(Box<Activity>),
}

#[derive(Debug, Deserialize)]
struct Incoming {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

/// Settings for one realtime connection.
#[derive(Debug, Clone)]
pub struct AstroConfig {
    pub url: String,
    pub jwt: String,
    pub channel_id: String,
}

impl AstroConfig {
    pub fn new(jwt: impl Into<String>, channel_id: impl Into<String>) -> Self {
        Self { url: ASTRO_URL.into(), jwt: jwt.into(), channel_id: channel_id.into() }
    }
}

enum Outcome {
    /// Reconnect after a backoff delay.
    Retry(String),
    /// Server asked us to move; reconnect now with this token.
    Resume(String),
    /// Stop for good.
    Stop,
}

/// Runs until `cancel` fires or the token is rejected, reconnecting with exponential backoff.
pub async fn run(cfg: AstroConfig, tx: mpsc::Sender<AstroEvent>, cancel: CancellationToken) {
    let mut backoff = Duration::from_secs(1);
    let mut resume: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            break;
        }
        let _ = tx.send(AstroEvent::Status(RealtimeStatus::Connecting)).await;

        let url = match resume.take() {
            Some(token) => match url::Url::parse_with_params(&cfg.url, &[("reconnect_token", token)]) {
                Ok(u) => (u.to_string(), true),
                Err(_) => (cfg.url.clone(), false),
            },
            None => (cfg.url.clone(), false),
        };

        let outcome = tokio::select! {
            _ = cancel.cancelled() => Outcome::Stop,
            o = session(&cfg, &url.0, url.1, &tx, &cancel, &mut backoff) => o,
        };

        match outcome {
            Outcome::Stop => break,
            Outcome::Resume(token) => {
                resume = Some(token);
                continue;
            }
            Outcome::Retry(reason) => {
                let _ = tx
                    .send(AstroEvent::Status(RealtimeStatus::Reconnecting {
                        in_secs: backoff.as_secs(),
                        reason,
                    }))
                    .await;
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
    let _ = tx.send(AstroEvent::Status(RealtimeStatus::Stopped)).await;
}

fn subscribe_frame(cfg: &AstroConfig, nonce: &str) -> String {
    json!({
        "type": "subscribe",
        "nonce": nonce,
        "data": {
            "topic": ACTIVITIES_TOPIC,
            "room": cfg.channel_id,
            "token": cfg.jwt,
            "token_type": "jwt"
        }
    })
    .to_string()
}

async fn session(
    cfg: &AstroConfig,
    url: &str,
    resumed: bool,
    tx: &mpsc::Sender<AstroEvent>,
    cancel: &CancellationToken,
    backoff: &mut Duration,
) -> Outcome {
    let mut ws = match tokio_tungstenite::connect_async(url).await {
        Ok((ws, _)) => ws,
        Err(e) => return Outcome::Retry(format!("could not connect: {e}")),
    };

    if !resumed {
        if let Err(e) = ws.send(Message::Text(subscribe_frame(cfg, "pine-activities"))).await {
            return Outcome::Retry(format!("could not subscribe: {e}"));
        }
    }

    loop {
        let msg = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = ws.close(None).await;
                return Outcome::Stop;
            }
            m = ws.next() => m,
        };
        let text = match msg {
            None => return Outcome::Retry("connection closed".into()),
            Some(Err(e)) => return Outcome::Retry(format!("connection error: {e}")),
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(Message::Close(frame))) => {
                let why = frame.map(|f| f.reason.to_string()).unwrap_or_default();
                return Outcome::Retry(format!("server closed the connection {why}").trim().to_string());
            }
            // Ping is answered automatically; pong, binary and raw frames are ignored.
            Some(Ok(_)) => continue,
        };

        let incoming: Incoming = match serde_json::from_str(&text) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("astro: ignoring unreadable frame: {e}");
                continue;
            }
        };

        match incoming.kind.as_str() {
            "welcome" => {
                let _ = tx.send(AstroEvent::Status(RealtimeStatus::Connected)).await;
                if resumed {
                    // Subscriptions are restored by the server on a reconnect_token connection.
                    *backoff = Duration::from_secs(1);
                    let _ = tx.send(AstroEvent::Status(RealtimeStatus::Live)).await;
                }
            }
            "response" => {
                let message = incoming
                    .data
                    .as_ref()
                    .and_then(|d| d.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                match incoming.error.as_deref() {
                    None => {
                        *backoff = Duration::from_secs(1);
                        let _ = tx.send(AstroEvent::Status(RealtimeStatus::Live)).await;
                    }
                    Some("err_unauthorized") => {
                        let _ = tx
                            .send(AstroEvent::Status(RealtimeStatus::Unauthorized {
                                message: if message.is_empty() { "token rejected".into() } else { message },
                            }))
                            .await;
                        let _ = ws.close(None).await;
                        return Outcome::Stop;
                    }
                    Some("err_deadline_exceeded") | Some("rate_limit_exceeded") | Some("err_internal_error") => {
                        let _ = ws.close(None).await;
                        return Outcome::Retry(format!("subscribe failed ({}), retrying", incoming.error.unwrap_or_default()));
                    }
                    Some(code) => {
                        // err_bad_request / invalid_message_type: retrying the same frame will not help.
                        let _ = tx
                            .send(AstroEvent::Status(RealtimeStatus::Unauthorized {
                                message: format!("subscribe rejected ({code}): {message}"),
                            }))
                            .await;
                        let _ = ws.close(None).await;
                        return Outcome::Stop;
                    }
                }
            }
            "message" if incoming.topic.as_deref() == Some(ACTIVITIES_TOPIC) => {
                if let Some(a) = incoming.data.as_ref().and_then(Activity::from_value) {
                    if tx.send(AstroEvent::Activity(Box::new(a))).await.is_err() {
                        return Outcome::Stop;
                    }
                } else {
                    log::warn!("astro: activity without id or type ignored");
                }
            }
            "reconnect" => {
                let token = incoming
                    .data
                    .as_ref()
                    .and_then(|d| d.get("reconnect_token"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let _ = ws.close(None).await;
                return match token {
                    Some(t) => Outcome::Resume(t),
                    None => Outcome::Retry("server asked to reconnect".into()),
                };
            }
            other => log::debug!("astro: unhandled frame type {other}"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)] // tungstenite's handshake callback signature
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    async fn next_event(rx: &mut mpsc::Receiver<AstroEvent>) -> AstroEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("timed out").expect("closed")
    }

    async fn wait_for_activity(rx: &mut mpsc::Receiver<AstroEvent>) -> Activity {
        loop {
            if let AstroEvent::Activity(a) = next_event(rx).await {
                return *a;
            }
        }
    }

    async fn wait_for_status(rx: &mut mpsc::Receiver<AstroEvent>, want: fn(&RealtimeStatus) -> bool) -> RealtimeStatus {
        loop {
            if let AstroEvent::Status(s) = next_event(rx).await {
                if want(&s) {
                    return s;
                }
            }
        }
    }

    #[tokio::test]
    async fn subscribes_delivers_activity_and_resumes_with_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (seen_tx, mut seen_rx) = mpsc::channel::<(String, Option<Value>)>(8);

        tokio::spawn(async move {
            // First connection: expect subscribe, ack, send activity, then ask to reconnect.
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(json!({"type":"welcome","data":{"client_id":"c1"}}).to_string())).await.unwrap();
            let sub = ws.next().await.unwrap().unwrap().into_text().unwrap();
            let v: Value = serde_json::from_str(&sub).unwrap();
            seen_tx.send(("first".into(), Some(v.clone()))).await.unwrap();
            ws.send(Message::Text(json!({"type":"response","nonce":v["nonce"],"data":{"message":"successfully subscribed to topic"}}).to_string())).await.unwrap();
            ws.send(Message::Ping(vec![1, 2, 3])).await.unwrap();
            ws.send(Message::Text(json!({"type":"message","topic":"channel.activities","data":{
                "_id":"a1","type":"cheer","provider":"twitch","data":{"username":"xx","amount":100,"message":"Cheer100"}}}).to_string())).await.unwrap();
            // The client must answer the ping with a pong.
            loop {
                match ws.next().await.unwrap().unwrap() {
                    Message::Pong(p) => { assert_eq!(p, vec![1, 2, 3]); break; }
                    _ => continue,
                }
            }
            ws.send(Message::Text(json!({"type":"reconnect","data":{"message":"The server is shutting down.","reconnect_token":"rt-1"}}).to_string())).await.unwrap();
            let _ = ws.next().await;

            // Second connection: must carry reconnect_token and must NOT re-subscribe.
            let (stream, _) = listener.accept().await.unwrap();
            let (path_tx, path_rx) = std::sync::mpsc::channel::<String>();
            let cb2 = move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                            resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                path_tx.send(req.uri().to_string()).unwrap();
                Ok(resp)
            };
            let mut ws = tokio_tungstenite::accept_hdr_async(stream, cb2).await.unwrap();
            let path = path_rx.recv().unwrap();
            seen_tx.send((path.clone(), None)).await.unwrap();
            ws.send(Message::Text(json!({"type":"welcome","data":{"client_id":"c2"}}).to_string())).await.unwrap();
            ws.send(Message::Text(json!({"type":"message","topic":"channel.activities","data":{
                "_id":"a2","type":"follow","data":{"username":"newfollower"}}}).to_string())).await.unwrap();
            // Anything the client sends now would be a re-subscribe; report it.
            if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_millis(300), ws.next()).await {
                seen_tx.send(("unexpected".into(), Some(Value::String(t.to_string())))).await.unwrap();
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        });

        let (tx, mut rx) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        let cfg = AstroConfig { url: format!("ws://{addr}"), jwt: "jwt-123".into(), channel_id: "chan-1".into() };
        let handle = tokio::spawn(run(cfg, tx, cancel.clone()));

        let (_, sub) = seen_rx.recv().await.unwrap();
        let sub = sub.unwrap();
        assert_eq!(sub["type"], "subscribe");
        assert_eq!(sub["data"]["topic"], "channel.activities");
        assert_eq!(sub["data"]["room"], "chan-1");
        assert_eq!(sub["data"]["token"], "jwt-123");
        assert_eq!(sub["data"]["token_type"], "jwt");

        wait_for_status(&mut rx, |s| *s == RealtimeStatus::Live).await;
        let a = wait_for_activity(&mut rx).await;
        assert_eq!(a.id, "a1");
        assert_eq!(a.amount, Some(100.0));

        let (path, _) = seen_rx.recv().await.unwrap();
        assert!(path.contains("reconnect_token=rt-1"), "path was {path}");
        let a2 = wait_for_activity(&mut rx).await;
        assert_eq!(a2.id, "a2");

        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(seen_rx.try_recv().is_err(), "client re-subscribed after a resume");

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stops_on_unauthorized() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let sub = ws.next().await.unwrap().unwrap().into_text().unwrap();
            let v: Value = serde_json::from_str(&sub).unwrap();
            ws.send(Message::Text(json!({"type":"response","nonce":v["nonce"],"error":"err_unauthorized","data":{"message":"bad token"}}).to_string())).await.unwrap();
            let _ = ws.next().await;
        });

        let (tx, mut rx) = mpsc::channel(32);
        let cfg = AstroConfig { url: format!("ws://{addr}"), jwt: "bad".into(), channel_id: "c".into() };
        let handle = tokio::spawn(run(cfg, tx, CancellationToken::new()));
        let s = wait_for_status(&mut rx, |s| matches!(s, RealtimeStatus::Unauthorized { .. })).await;
        assert_eq!(s, RealtimeStatus::Unauthorized { message: "bad token".into() });
        wait_for_status(&mut rx, |s| *s == RealtimeStatus::Stopped).await;
        tokio::time::timeout(Duration::from_secs(5), handle).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn retries_when_server_is_down() {
        // Bind then drop to get a port nobody listens on.
        let addr = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        let (tx, mut rx) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        let cfg = AstroConfig { url: format!("ws://{addr}"), jwt: "j".into(), channel_id: "c".into() };
        let handle = tokio::spawn(run(cfg, tx, cancel.clone()));
        let s = wait_for_status(&mut rx, |s| matches!(s, RealtimeStatus::Reconnecting { .. })).await;
        assert!(matches!(s, RealtimeStatus::Reconnecting { in_secs: 1, .. }));
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle).await.unwrap().unwrap();
    }
}
