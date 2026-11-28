//! The few Spotify Web API calls the song player needs: what is playing,
//! play a track, pause, resume, seek and volume.
//!
//! Sylva Control never signs in to Spotify itself. The streamer connects
//! Spotify once on Sylva's Song requests page, and this asks Sylva for a short
//! lived access token (`songs/spotify/token`) whenever it needs one. Playback
//! through the Web API needs Spotify Premium and an open Spotify app (on this
//! PC or any device on the account).

use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};

use crate::error::Error;
use crate::sylva::SylvaClient;

const API: &str = "https://api.spotify.com/v1";

/// Why Spotify would not do what was asked, worded for the player's status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyError {
    /// Not connected, or the sign in stopped working. Carries Sylva's words.
    NotConnected(String),
    Premium,
    /// No Spotify app is open on the account.
    NoDevice,
    /// Anything else: Spotify busy, offline, or a refusal.
    Other(String),
}

impl std::fmt::Display for SpotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpotifyError::NotConnected(m) => f.write_str(m),
            SpotifyError::Premium => f.write_str("Spotify only lets apps play songs on Premium accounts"),
            SpotifyError::NoDevice => f.write_str("Open Spotify on this PC, then press play"),
            SpotifyError::Other(m) => f.write_str(m),
        }
    }
}

type Result<T> = std::result::Result<T, SpotifyError>;

/// What Spotify says is playing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Now {
    /// The track id, when a track (not an episode or ad) is loaded.
    pub track: Option<String>,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// The device's volume, when it has one.
    pub volume: Option<u8>,
}

/// Read `GET /me/player`.
pub fn parse_now(v: &Value) -> Now {
    let item = &v["item"];
    Now {
        track: item["id"].as_str().filter(|_| item["type"].as_str() != Some("episode")).map(str::to_string),
        playing: v["is_playing"].as_bool().unwrap_or(false),
        position_ms: v["progress_ms"].as_u64().unwrap_or(0),
        duration_ms: item["duration_ms"].as_u64().unwrap_or(0),
        volume: v["device"]["volume_percent"].as_u64().map(|n| n.min(100) as u8),
    }
}

/// The device to play on when none is active: this computer if Spotify is
/// open here, else the first one that takes commands.
pub fn pick_device(v: &Value) -> Option<String> {
    let list = v["devices"].as_array()?;
    let usable = |d: &&Value| d["is_restricted"].as_bool() != Some(true) && d["id"].as_str().is_some();
    list.iter()
        .filter(usable)
        .find(|d| d["is_active"].as_bool() == Some(true))
        .or_else(|| list.iter().filter(usable).find(|d| d["type"].as_str() == Some("Computer")))
        .or_else(|| list.iter().find(usable))
        .and_then(|d| d["id"].as_str().map(str::to_string))
}

pub struct Spotify {
    http: reqwest::Client,
    sylva: SylvaClient,
    key: String,
    token: Option<String>,
}

impl Spotify {
    pub fn new(sylva: SylvaClient, key: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Spotify { http, sylva, key, token: None }
    }

    async fn token(&mut self, fresh: bool) -> Result<String> {
        if !fresh {
            if let Some(t) = &self.token {
                return Ok(t.clone());
            }
        }
        match self.sylva.spotify_token(&self.key, fresh).await {
            Ok((t, _)) => {
                self.token = Some(t.clone());
                Ok(t)
            }
            Err(Error::Http { status: 409, body }) => Err(SpotifyError::NotConnected(body)),
            Err(e) => Err(SpotifyError::Other(format!("Could not reach Sylva: {e}"))),
        }
    }

    /// One call, with a new token once if Spotify turns the old one down.
    async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut fresh = false;
        loop {
            let token = self.token(fresh).await?;
            let mut req = self.http.request(method.clone(), format!("{API}{path}")).bearer_auth(&token);
            req = match &body {
                Some(b) => req.json(b),
                // Spotify wants a length on bodiless PUTs.
                None if method != Method::GET => req.header("Content-Length", "0"),
                None => req,
            };
            let resp = req.send().await.map_err(|e| {
                SpotifyError::Other(if e.is_timeout() { "Spotify timed out".into() } else { "Spotify could not be reached".into() })
            })?;
            let status = resp.status().as_u16();
            let v: Value = resp.json().await.unwrap_or(Value::Null);
            let reason = v["error"]["reason"].as_str().unwrap_or("");
            match status {
                200..=299 => return Ok(v),
                401 if !fresh => {
                    self.token = None;
                    fresh = true;
                }
                401 => return Err(SpotifyError::NotConnected("Spotify signed Sylva out. Connect Spotify again on the Song requests page".into())),
                403 if reason == "PREMIUM_REQUIRED" => return Err(SpotifyError::Premium),
                404 if reason == "NO_ACTIVE_DEVICE" || path.starts_with("/me/player") => return Err(SpotifyError::NoDevice),
                429 => return Err(SpotifyError::Other("Spotify asked to slow down".into())),
                _ => {
                    let m = v["error"]["message"].as_str().unwrap_or("it said no");
                    return Err(SpotifyError::Other(format!("Spotify refused ({status}): {m}")));
                }
            }
        }
    }

    pub async fn now(&mut self) -> Result<Now> {
        // 204 (nothing is playing anywhere) comes back as an empty value.
        Ok(parse_now(&self.call(Method::GET, "/me/player", None).await?))
    }

    /// Play one track from `position_ms`, on the active device or, when
    /// none is active, the best one on the account.
    pub async fn play(&mut self, track: &str, position_ms: u64) -> Result<()> {
        let body = json!({ "uris": [format!("spotify:track:{track}")], "position_ms": position_ms });
        match self.call(Method::PUT, "/me/player/play", Some(body.clone())).await {
            Err(SpotifyError::NoDevice) => {
                let devices = self.call(Method::GET, "/me/player/devices", None).await?;
                let id = pick_device(&devices).ok_or(SpotifyError::NoDevice)?;
                self.call(Method::PUT, &format!("/me/player/play?device_id={id}"), Some(body)).await.map(|_| ())
            }
            r => r.map(|_| ()),
        }
    }

    pub async fn resume(&mut self) -> Result<()> {
        self.call(Method::PUT, "/me/player/play", None).await.map(|_| ())
    }

    pub async fn pause(&mut self) -> Result<()> {
        self.call(Method::PUT, "/me/player/pause", None).await.map(|_| ())
    }

    pub async fn seek(&mut self, position_ms: u64) -> Result<()> {
        self.call(Method::PUT, &format!("/me/player/seek?position_ms={position_ms}"), None).await.map(|_| ())
    }

    pub async fn volume(&mut self, volume: u8) -> Result<()> {
        self.call(Method::PUT, &format!("/me/player/volume?volume_percent={}", volume.min(100)), None).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_player() {
        let v = json!({
            "is_playing": true, "progress_ms": 1200,
            "device": { "volume_percent": 55 },
            "item": { "id": "4uLU6hMCjMI75M1A2tKUQC", "type": "track", "duration_ms": 213573 }
        });
        let n = parse_now(&v);
        assert_eq!(n.track.as_deref(), Some("4uLU6hMCjMI75M1A2tKUQC"));
        assert_eq!((n.playing, n.position_ms, n.duration_ms, n.volume), (true, 1200, 213573, Some(55)));
        assert_eq!(parse_now(&Value::Null), Now::default());
    }

    #[test]
    fn picks_a_device() {
        let v = json!({ "devices": [
            { "id": "phone", "type": "Smartphone", "is_active": false },
            { "id": "web", "type": "Computer", "is_restricted": true },
            { "id": "pc", "type": "Computer", "is_active": false }
        ] });
        assert_eq!(pick_device(&v).as_deref(), Some("pc"));
        let v = json!({ "devices": [{ "id": "phone", "type": "Smartphone", "is_active": true }, { "id": "pc", "type": "Computer" }] });
        assert_eq!(pick_device(&v).as_deref(), Some("phone"));
        assert_eq!(pick_device(&json!({ "devices": [] })), None);
    }
}
