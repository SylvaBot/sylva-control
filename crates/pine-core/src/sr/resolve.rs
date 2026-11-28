//! Turn a YouTube video id into its audio, as bytes in memory.
//!
//! YouTube's own apps ask `youtubei/v1/player` for a video and get direct
//! links to each audio and video stream. Sylva Control asks the same way, as
//! two of those apps (Android VR, then iOS), whose answers carry plain links
//! that need no script from YouTube's web player to unlock. It takes only the
//! audio stream (AAC in MP4, itag 140, about 1 MB a minute), so no video is
//! ever downloaded, and no ad is either: ads are separate videos the apps
//! choose to play, never part of the stream.
//!
//! The links only work from the address that asked for them, so the app
//! resolves and downloads on the streamer's own PC. It downloads the whole
//! song in 1 MiB pieces (`&range=`, as YouTube's apps do) before playing:
//! a 4 minute song is about 4 MB, seeking is instant, and a network hiccup
//! mid song cannot stop the music.
//!
//! **Risk:** this is not an official API and YouTube's terms do not allow
//! playing its audio outside its own players. YouTube changes these clients
//! from time to time, and then lookups fail until the client names and
//! versions below are updated. Everything YouTube specific is in this file.

use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::time::Duration;

const PIECE: u64 = 1 << 20;
/// Longer than any song the server lets through (one hour is about 60 MB).
const MAX_BYTES: u64 = 120 << 20;

struct Client {
    name: &'static str,
    id: &'static str,
    version: &'static str,
    agent: &'static str,
    extra: fn() -> Value,
}

const CLIENTS: &[Client] = &[
    Client {
        name: "ANDROID_VR",
        id: "28",
        version: "1.65.10",
        agent: "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
        extra: || json!({ "deviceMake": "Oculus", "deviceModel": "Quest 3", "androidSdkVersion": 32, "osName": "Android", "osVersion": "12L" }),
    },
    Client {
        name: "IOS",
        id: "5",
        version: "20.10.4",
        agent: "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
        extra: || json!({ "deviceMake": "Apple", "deviceModel": "iPhone16,2", "osName": "iPhone", "osVersion": "18.3.2.22D82" }),
    },
];

/// A playable audio stream.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioStream {
    pub url: String,
    pub mime: String,
    pub bytes: u64,
    /// The client that found it; the download uses the same identity.
    agent: &'static str,
}

/// Pick the audio stream to play from a `player` answer: AAC (which the
/// built-in decoder plays), the 128 kbps one when there is a choice.
pub fn pick_audio(v: &Value) -> std::result::Result<(String, String, u64), String> {
    let status = v["playabilityStatus"]["status"].as_str().unwrap_or("");
    if status != "OK" {
        let reason = v["playabilityStatus"]["reason"].as_str().unwrap_or("YouTube would not play it");
        return Err(reason.to_string());
    }
    let formats = v["streamingData"]["adaptiveFormats"].as_array().cloned().unwrap_or_default();
    let mut best: Option<(i64, String, String, u64)> = None;
    for f in &formats {
        let mime = f["mimeType"].as_str().unwrap_or("");
        let Some(url) = f["url"].as_str() else { continue };
        if !mime.starts_with("audio/mp4") {
            continue;
        }
        let rank = match f["itag"].as_i64() {
            Some(140) => 3,
            Some(139) => 1,
            _ => 2,
        };
        let bytes = f["contentLength"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
        if best.as_ref().is_none_or(|b| rank > b.0) {
            best = Some((rank, url.to_string(), mime.to_string(), bytes));
        }
    }
    best.map(|(_, u, m, b)| (u, m, b)).ok_or_else(|| "YouTube sent no audio this app can play".into())
}

/// Find the audio stream for a video, trying each client in turn.
pub async fn resolve(http: &reqwest::Client, video_id: &str) -> Result<AudioStream> {
    let mut last = String::from("YouTube did not answer");
    for c in CLIENTS {
        let mut client = json!({ "clientName": c.name, "clientVersion": c.version, "hl": "en", "gl": "US" });
        if let (Some(o), Some(extra)) = (client.as_object_mut(), (c.extra)().as_object()) {
            o.extend(extra.clone());
        }
        let body = json!({ "context": { "client": client }, "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true });
        let resp = http
            .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("User-Agent", c.agent)
            .header("X-Youtube-Client-Name", c.id)
            .header("X-Youtube-Client-Version", c.version)
            .timeout(Duration::from_secs(15))
            .json(&body)
            .send()
            .await;
        let v: Value = match resp {
            Ok(r) if r.status().is_success() => match r.json().await {
                Ok(v) => v,
                Err(e) => {
                    last = e.to_string();
                    continue;
                }
            },
            Ok(r) => {
                last = format!("YouTube answered {}", r.status().as_u16());
                continue;
            }
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        match pick_audio(&v) {
            Ok((url, mime, bytes)) => return Ok(AudioStream { url, mime, bytes, agent: c.agent }),
            Err(m) => last = m,
        }
    }
    Err(Error::Other(last))
}

/// Download a stream whole, in pieces. Stops early when `cancel` fires.
pub async fn download(http: &reqwest::Client, s: &AudioStream, cancel: &tokio_util::sync::CancellationToken) -> Result<Vec<u8>> {
    if s.bytes > MAX_BYTES {
        return Err(Error::Other("that song is too long to load".into()));
    }
    let mut out = Vec::with_capacity(s.bytes as usize);
    let mut at = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(Error::Other("canceled".into()));
        }
        let end = at + PIECE - 1;
        let mut tries = 0;
        let piece = loop {
            let r = http
                .get(format!("{}&range={at}-{end}", s.url))
                .header("User-Agent", s.agent)
                .timeout(Duration::from_secs(30))
                .send()
                .await;
            match r {
                Ok(r) if r.status().is_success() => match r.bytes().await {
                    Ok(b) => break b,
                    Err(e) if tries >= 2 => return Err(e.into()),
                    Err(_) => {}
                },
                Ok(r) if tries >= 2 => {
                    return Err(Error::Http { status: r.status().as_u16(), body: "YouTube refused the audio".into() })
                }
                Err(e) if tries >= 2 => return Err(e.into()),
                _ => {}
            }
            tries += 1;
            tokio::time::sleep(Duration::from_millis(400 * tries)).await;
        };
        let n = piece.len() as u64;
        out.extend_from_slice(&piece);
        at += n;
        let done = if s.bytes > 0 { at >= s.bytes } else { n < PIECE };
        if done || n == 0 {
            break;
        }
        if at > MAX_BYTES {
            return Err(Error::Other("that song is too long to load".into()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_aac_128() {
        let v = json!({
            "playabilityStatus": { "status": "OK" },
            "streamingData": { "adaptiveFormats": [
                { "itag": 137, "mimeType": "video/mp4", "url": "v" },
                { "itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "url": "low", "contentLength": "10" },
                { "itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "url": "opus" },
                { "itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "url": "aac", "contentLength": "3449447" }
            ] }
        });
        assert_eq!(pick_audio(&v).unwrap(), ("aac".into(), "audio/mp4; codecs=\"mp4a.40.2\"".into(), 3449447));
        let no = json!({ "playabilityStatus": { "status": "LOGIN_REQUIRED", "reason": "Sign in" } });
        assert_eq!(pick_audio(&no).unwrap_err(), "Sign in");
    }
}
