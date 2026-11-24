//! Twitch sign-in (device code flow, no client secret) and the Helix calls behind the
//! viewers, chatters, subscribers and followers panels. StreamElements does not expose
//! live viewer or chatter lists, so these come straight from Twitch.
//!
//! References: dev.twitch.tv/docs/authentication/getting-tokens-oauth (device code grant),
//! dev.twitch.tv/docs/authentication/refresh-tokens, dev.twitch.tv/docs/authentication/validate-tokens,
//! dev.twitch.tv/docs/api/reference (Get Streams, Get Chatters, Get Broadcaster Subscriptions,
//! Get Channel Followers).

use crate::error::{check, Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const ID_BASE: &str = "https://id.twitch.tv";
pub const API_BASE: &str = "https://api.twitch.tv/helix";
pub const SCOPES: &str = "channel:read:subscriptions moderator:read:followers moderator:read:chatters";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TwitchTokens {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub user_id: String,
    pub login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    user_id: Option<String>,
    login: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chatter {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscriber {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub tier: String,
    pub is_gift: bool,
    #[serde(default)]
    pub gifter_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Follower {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub followed_at: String,
}

#[derive(Debug, Deserialize)]
struct HelixPage<T> {
    data: Vec<T>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

impl<T> From<HelixPage<T>> for Page<T> {
    fn from(p: HelixPage<T>) -> Self {
        Page {
            items: p.data,
            total: p.total,
            cursor: p.pagination.and_then(|p| p.cursor).filter(|c| !c.is_empty()),
        }
    }
}

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Pine/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()?)
}

/// Step 1 of sign-in: ask Twitch for a code the user enters at `verification_uri`.
pub async fn start_device_flow(http: &reqwest::Client, id_base: &str, client_id: &str) -> Result<DeviceCode> {
    let resp = http
        .post(format!("{id_base}/oauth2/device"))
        .form(&[("client_id", client_id), ("scopes", SCOPES)])
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// Step 2: poll until the user approves, the code expires, or `cancel` fires.
pub async fn finish_device_flow(
    http: &reqwest::Client,
    id_base: &str,
    client_id: &str,
    code: &DeviceCode,
    cancel: &CancellationToken,
) -> Result<TwitchTokens> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let interval = Duration::from_secs(code.interval.max(1));
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Other("the Twitch sign-in code expired, please try again".into()));
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Other("Twitch sign-in was cancelled".into())),
            _ = tokio::time::sleep(interval) => {}
        }
        let resp = http
            .post(format!("{id_base}/oauth2/token"))
            .form(&[
                ("client_id", client_id),
                ("scopes", SCOPES),
                ("device_code", code.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;
        if resp.status().as_u16() == 400 {
            let body: Value = resp.json().await.unwrap_or(Value::Null);
            if body.get("message").and_then(Value::as_str) == Some("authorization_pending") {
                continue;
            }
            return Err(Error::Http { status: 400, body: body.to_string() });
        }
        let tok: TokenResponse = check(resp).await?.json().await?;
        let (user_id, login) = validate(http, id_base, &tok.access_token).await?;
        return Ok(TwitchTokens {
            client_id: client_id.to_string(),
            access_token: tok.access_token,
            refresh_token: tok.refresh_token,
            user_id,
            login,
        });
    }
}

/// `GET /oauth2/validate`: returns `(user_id, login)` for a user token.
pub async fn validate(http: &reqwest::Client, id_base: &str, access_token: &str) -> Result<(String, String)> {
    let resp = http
        .get(format!("{id_base}/oauth2/validate"))
        .header("Authorization", format!("OAuth {access_token}"))
        .send()
        .await?;
    let v: ValidateResponse = check(resp).await?.json().await?;
    match (v.user_id, v.login) {
        (Some(id), Some(login)) => Ok((id, login)),
        _ => Err(Error::Other("Twitch returned an app token; a user token is required".into())),
    }
}

type RefreshHook = Box<dyn Fn(&TwitchTokens) + Send + Sync>;

/// Helix client that refreshes its token once on a 401 and reports new tokens via `on_refresh`.
pub struct TwitchClient {
    http: reqwest::Client,
    tokens: Mutex<TwitchTokens>,
    on_refresh: Option<RefreshHook>,
    id_base: String,
    api_base: String,
}

impl TwitchClient {
    pub fn new(tokens: TwitchTokens, on_refresh: Option<RefreshHook>) -> Result<Arc<Self>> {
        Self::with_bases(tokens, on_refresh, ID_BASE, API_BASE)
    }

    pub fn with_bases(
        tokens: TwitchTokens,
        on_refresh: Option<RefreshHook>,
        id_base: &str,
        api_base: &str,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            http: http_client()?,
            tokens: Mutex::new(tokens),
            on_refresh,
            id_base: id_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        }))
    }

    pub async fn tokens(&self) -> TwitchTokens {
        self.tokens.lock().await.clone()
    }

    async fn refresh(&self) -> Result<()> {
        let mut t = self.tokens.lock().await;
        let Some(rt) = t.refresh_token.clone() else {
            return Err(Error::Unauthorized { status: 401, body: "Twitch session expired".into() });
        };
        let resp = self
            .http
            .post(format!("{}/oauth2/token", self.id_base))
            .form(&[
                ("client_id", t.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", rt.as_str()),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 400 || status == 401 {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Unauthorized { status, body });
        }
        let tok: TokenResponse = check(resp).await?.json().await?;
        t.access_token = tok.access_token;
        if tok.refresh_token.is_some() {
            t.refresh_token = tok.refresh_token;
        }
        if let Some(hook) = &self.on_refresh {
            hook(&t);
        }
        Ok(())
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        for attempt in 0..2 {
            let t = self.tokens().await;
            let resp = self
                .http
                .get(format!("{}{}", self.api_base, path))
                .query(query)
                .header("Authorization", format!("Bearer {}", t.access_token))
                .header("Client-Id", &t.client_id)
                .send()
                .await?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                self.refresh().await?;
                continue;
            }
            return Ok(check(resp).await?.json().await?);
        }
        unreachable!("the loop returns on its second attempt")
    }

    fn page_query(user_id: &str, first: u32, after: Option<&str>, as_moderator: bool) -> Vec<(&'static str, String)> {
        let mut q = vec![("broadcaster_id", user_id.to_string()), ("first", first.to_string())];
        if as_moderator {
            // Get Chatters requires moderator_id; the broadcaster counts as their own moderator.
            q.push(("moderator_id", user_id.to_string()));
        }
        if let Some(a) = after {
            q.push(("after", a.to_string()));
        }
        q
    }

    /// Live viewer count, or `None` when the channel is offline.
    pub async fn viewer_count(&self) -> Result<Option<u64>> {
        let uid = self.tokens().await.user_id;
        let v: Value = self.get("/streams", &[("user_id", uid)]).await?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|s| s.get("viewer_count"))
            .and_then(Value::as_u64))
    }

    /// Users in chat right now (scope `moderator:read:chatters`).
    pub async fn chatters(&self, after: Option<&str>) -> Result<Page<Chatter>> {
        let uid = self.tokens().await.user_id;
        let q = Self::page_query(&uid, 1000, after, true);
        Ok(self.get::<HelixPage<Chatter>>("/chat/chatters", &q).await?.into())
    }

    /// Current subscribers (scope `channel:read:subscriptions`).
    pub async fn subscribers(&self, after: Option<&str>) -> Result<Page<Subscriber>> {
        let uid = self.tokens().await.user_id;
        let q = Self::page_query(&uid, 100, after, false);
        Ok(self.get::<HelixPage<Subscriber>>("/subscriptions", &q).await?.into())
    }

    /// Followers, newest first (scope `moderator:read:followers`).
    pub async fn followers(&self, after: Option<&str>) -> Result<Page<Follower>> {
        let uid = self.tokens().await.user_id;
        let q = Self::page_query(&uid, 100, after, false);
        Ok(self.get::<HelixPage<Follower>>("/channels/followers", &q).await?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::mock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tokens() -> TwitchTokens {
        TwitchTokens {
            client_id: "cid".into(),
            access_token: "old".into(),
            refresh_token: Some("r1".into()),
            user_id: "42".into(),
            login: "streamer".into(),
        }
    }

    #[tokio::test]
    async fn refreshes_once_on_401_and_reports_new_tokens() {
        let base = mock(|head, body| {
            let first = head.lines().next().unwrap();
            if first.starts_with("POST /oauth2/token") {
                assert!(body.contains("grant_type=refresh_token"));
                assert!(body.contains("refresh_token=r1"));
                assert!(body.contains("client_id=cid"));
                assert!(!body.contains("client_secret"));
                return (200, r#"{"access_token":"new","refresh_token":"r2","scope":[],"token_type":"bearer"}"#.into());
            }
            if first.starts_with("GET /helix/streams?user_id=42") {
                assert!(head.contains("client-id: cid") || head.contains("Client-Id: cid"));
                if head.contains("Bearer old") {
                    return (401, r#"{"status":401,"message":"invalid access token"}"#.into());
                }
                assert!(head.contains("Bearer new"));
                return (200, r#"{"data":[{"user_id":"42","viewer_count":17}],"pagination":{}}"#.into());
            }
            (404, "{}".into())
        })
        .await;

        let saved = Arc::new(std::sync::Mutex::new(None));
        let saved2 = saved.clone();
        let c = TwitchClient::with_bases(
            tokens(),
            Some(Box::new(move |t: &TwitchTokens| *saved2.lock().unwrap() = Some(t.clone()))),
            &base,
            &format!("{base}/helix"),
        )
        .unwrap();
        assert_eq!(c.viewer_count().await.unwrap(), Some(17));
        let s = saved.lock().unwrap().clone().unwrap();
        assert_eq!(s.access_token, "new");
        assert_eq!(s.refresh_token.as_deref(), Some("r2"));
    }

    #[tokio::test]
    async fn offline_channel_has_no_viewer_count_and_lists_page() {
        let base = mock(|head, _| {
            let first = head.lines().next().unwrap();
            if first.starts_with("GET /helix/streams") {
                return (200, r#"{"data":[],"pagination":{}}"#.into());
            }
            if first.starts_with("GET /helix/chat/chatters?broadcaster_id=42&first=1000&moderator_id=42 ") {
                return (200, r#"{"data":[{"user_id":"1","user_login":"a","user_name":"A"}],"pagination":{"cursor":"next1"},"total":8}"#.into());
            }
            if first.starts_with("GET /helix/subscriptions?broadcaster_id=42&first=100&after=c9 ") {
                return (200, r#"{"data":[{"broadcaster_id":"42","broadcaster_login":"s","broadcaster_name":"S","gifter_id":"","gifter_login":"","gifter_name":"","is_gift":false,"plan_name":"Tier 1","tier":"1000","user_id":"7","user_name":"Bob","user_login":"bob"}],"pagination":{},"total":5,"points":5}"#.into());
            }
            if first.starts_with("GET /helix/channels/followers?broadcaster_id=42&first=100 ") {
                return (200, r#"{"total":757,"data":[{"user_id":"9","user_name":"Fan","user_login":"fan","followed_at":"2026-11-24T00:00:00Z"}],"pagination":{}}"#.into());
            }
            (404, format!("{{\"unexpected\":\"{first}\"}}"))
        })
        .await;
        let c = TwitchClient::with_bases(tokens(), None, &base, &format!("{base}/helix")).unwrap();
        assert_eq!(c.viewer_count().await.unwrap(), None);

        let ch = c.chatters(None).await.unwrap();
        assert_eq!(ch.total, Some(8));
        assert_eq!(ch.cursor.as_deref(), Some("next1"));
        assert_eq!(ch.items[0].user_name, "A");

        let subs = c.subscribers(Some("c9")).await.unwrap();
        assert_eq!(subs.total, Some(5));
        assert_eq!(subs.cursor, None);
        assert_eq!(subs.items[0].tier, "1000");

        let f = c.followers(None).await.unwrap();
        assert_eq!(f.total, Some(757));
        assert_eq!(f.items[0].user_login, "fan");
    }

    #[tokio::test]
    async fn device_flow_waits_while_pending_then_validates() {
        let polls = Arc::new(AtomicUsize::new(0));
        let polls2 = polls.clone();
        let base = mock(move |head, body| {
            let first = head.lines().next().unwrap();
            if first.starts_with("POST /oauth2/device") {
                assert!(body.contains("client_id=cid"));
                assert!(body.contains("scopes=channel%3Aread%3Asubscriptions"));
                return (200, r#"{"device_code":"dc","expires_in":60,"interval":1,"user_code":"ABCD-EFGH","verification_uri":"https://www.twitch.tv/activate?device-code=ABCDEFGH"}"#.into());
            }
            if first.starts_with("POST /oauth2/token") {
                assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));
                assert!(body.contains("device_code=dc"));
                if polls2.fetch_add(1, Ordering::SeqCst) == 0 {
                    return (400, r#"{"status":400,"message":"authorization_pending"}"#.into());
                }
                return (200, r#"{"access_token":"at","expires_in":14400,"refresh_token":"rt","scope":[],"token_type":"bearer"}"#.into());
            }
            if first.starts_with("GET /oauth2/validate") {
                assert!(head.contains("OAuth at"));
                return (200, r#"{"client_id":"cid","login":"streamer","scopes":[],"user_id":"42","expires_in":14000}"#.into());
            }
            (404, "{}".into())
        })
        .await;
        let http = http_client().unwrap();
        let code = start_device_flow(&http, &base, "cid").await.unwrap();
        assert_eq!(code.user_code, "ABCD-EFGH");
        let t = finish_device_flow(&http, &base, "cid", &code, &CancellationToken::new()).await.unwrap();
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert_eq!(t, TwitchTokens {
            client_id: "cid".into(),
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            user_id: "42".into(),
            login: "streamer".into(),
        });
    }
}
