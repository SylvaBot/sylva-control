//! Live viewer count without a Twitch sign-in.
//!
//! Uses DecAPI (https://decapi.me), a free public service built for stream bots. It answers
//! `GET /twitch/viewercount/{channel}` in plain text: the number while the channel is live,
//! or a sentence such as "name is offline" when it is not.

use crate::error::{check, Error, Result};
use std::time::Duration;

pub const DEFAULT_BASE: &str = "https://decapi.me";

#[derive(Clone)]
pub struct PublicViewers {
    http: reqwest::Client,
    base: String,
}

impl PublicViewers {
    pub fn new() -> Result<Self> {
        Self::with_base(DEFAULT_BASE)
    }

    pub fn with_base(base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Pine/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { http, base: base.trim_end_matches('/').into() })
    }

    /// `Ok(Some(n))` while live, `Ok(None)` when offline.
    pub async fn viewer_count(&self, login: &str) -> Result<Option<u64>> {
        let login = login.trim().to_ascii_lowercase();
        if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::Other(format!("\"{login}\" is not a Twitch channel name")));
        }
        let url = format!("{}/twitch/viewercount/{login}", self.base);
        let resp = check(self.http.get(url).send().await?).await?;
        parse(&resp.text().await?)
    }
}

/// The viewer count straight from the feed twitch.tv itself reads. It is not an official API, so
/// callers must fall back to the Helix or DecAPI paths when it fails.
#[derive(Clone)]
pub struct SiteViewers {
    http: reqwest::Client,
    url: String,
}

/// The public web client id twitch.tv uses for its own pages.
const SITE_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
const SITE_URL: &str = "https://gql.twitch.tv/gql";

impl SiteViewers {
    pub fn new() -> Result<Self> {
        Self::with_url(SITE_URL)
    }

    pub fn with_url(url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Pine/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self { http, url: url.into() })
    }

    /// `Ok(Some(n))` while live, `Ok(None)` when offline.
    pub async fn viewer_count(&self, login: &str) -> Result<Option<u64>> {
        let login = login.trim().to_ascii_lowercase();
        if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::Other(format!("\"{login}\" is not a Twitch channel name")));
        }
        let body = serde_json::json!({
            "query": format!("query {{ user(login: \"{login}\") {{ stream {{ viewersCount }} }} }}")
        });
        let resp = self.http.post(&self.url).header("Client-ID", SITE_CLIENT_ID).json(&body).send().await?;
        let resp = check(resp).await?;
        parse_site(&resp.text().await?)
    }
}

pub(crate) fn parse_site(body: &str) -> Result<Option<u64>> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| Error::Other(format!("bad site answer: {e}")))?;
    let user = v.pointer("/data/user").ok_or_else(|| Error::Other("site feed gave no user".into()))?;
    if user.is_null() {
        return Err(Error::Other("site feed does not know this channel".into()));
    }
    match user.pointer("/stream") {
        Some(s) if s.is_null() => Ok(None),
        Some(s) => s.get("viewersCount").and_then(serde_json::Value::as_u64).map(Some).ok_or_else(|| Error::Other("site feed gave no count".into())),
        None => Err(Error::Other("site feed gave no stream field".into())),
    }
}

pub(crate) fn parse(body: &str) -> Result<Option<u64>> {
    let text = body.trim();
    if let Ok(n) = text.replace(',', "").parse::<u64>() {
        return Ok(Some(n));
    }
    if text.to_ascii_lowercase().contains("offline") {
        return Ok(None);
    }
    let short: String = text.chars().take(200).collect();
    Err(Error::Other(if short.is_empty() { "empty answer from the viewer count service".into() } else { short }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::mock;

    #[test]
    fn parses_answers() {
        assert_eq!(parse("1234").unwrap(), Some(1234));
        assert_eq!(parse(" 0\n").unwrap(), Some(0));
        assert_eq!(parse("haruxee is offline").unwrap(), None);
        assert!(parse("User not found: nobody").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn parses_site_answers() {
        assert_eq!(parse_site(r#"{"data":{"user":{"stream":{"viewersCount":42}}}}"#).unwrap(), Some(42));
        assert_eq!(parse_site(r#"{"data":{"user":{"stream":null}}}"#).unwrap(), None);
        assert!(parse_site(r#"{"data":{"user":null}}"#).is_err());
        assert!(parse_site(r#"{"errors":[{"message":"x"}]}"#).is_err());
        assert!(parse_site("nope").is_err());
    }

    #[tokio::test]
    async fn fetches_count_for_channel() {
        let base = mock(|head, _| {
            let line = head.lines().next().unwrap();
            if line.starts_with("GET /twitch/viewercount/haruxee ") {
                (200, "57".into())
            } else if line.starts_with("GET /twitch/viewercount/sleepy ") {
                (200, "sleepy is offline".into())
            } else {
                (404, format!("unexpected {line}"))
            }
        })
        .await;
        let c = PublicViewers::with_base(&base).unwrap();
        assert_eq!(c.viewer_count("HaruXee").await.unwrap(), Some(57));
        assert_eq!(c.viewer_count("sleepy").await.unwrap(), None);
        assert!(c.viewer_count("bad/name").await.is_err());
    }
}
