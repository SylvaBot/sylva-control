//! The song request queue as Sylva Control sees it.
//!
//! Sylva holds the queue because chat, the dashboard and this app all change
//! it; this is the app's copy, updated from every `songs` frame. It answers
//! the questions the player asks: which song should be loaded, which one to
//! fetch ahead, and what to show.

use serde::{Deserialize, Serialize};

/// One requested song (`sylva_core::songs::Song` on the server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: u64,
    /// The Spotify track id. Empty on songs queued from YouTube before Spotify.
    #[serde(default)]
    pub track: String,
    #[serde(default)]
    pub video_id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub duration_s: u32,
    #[serde(default)]
    pub by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Kept as sent so a save from this app never drops a rule it does not show.
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// What the player last told Sylva (`Playback` on the server).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    #[serde(default)]
    pub song_id: Option<u64>,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub position_ms: u64,
    #[serde(default)]
    pub volume: u8,
    #[serde(default)]
    pub status: Option<String>,
}

/// Sylva's `SongsView`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongsView {
    #[serde(default)]
    pub settings: SongSettings,
    #[serde(default)]
    pub current: Option<Song>,
    #[serde(default)]
    pub queue: Vec<Song>,
    #[serde(default)]
    pub playback: Playback,
}

/// A remote press from the dashboard (`SongControl` on the server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SongControl {
    Play,
    Pause,
    #[serde(rename_all = "camelCase")]
    Seek { position_ms: u64 },
    Volume { volume: u8 },
}

/// The app's copy of the queue.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    view: SongsView,
    /// Whether any view has arrived yet.
    known: bool,
}

impl Queue {
    pub fn view(&self) -> &SongsView {
        &self.view
    }

    pub fn known(&self) -> bool {
        self.known
    }

    /// Take Sylva's latest view. Returns true when the song up now changed,
    /// which means the player should load it.
    pub fn update(&mut self, view: SongsView) -> bool {
        let before = self.view.current.as_ref().map(|s| s.id);
        let changed = !self.known || before != view.current.as_ref().map(|s| s.id);
        self.view = view;
        self.known = true;
        changed
    }

    pub fn current(&self) -> Option<&Song> {
        self.view.current.as_ref()
    }

    /// The song after this one.
    pub fn up_next(&self) -> Option<&Song> {
        self.view.queue.first()
    }

    pub fn enabled(&self) -> bool {
        self.view.settings.enabled
    }

    /// The volume to start at: what the player last reported, else 60.
    pub fn last_volume(&self) -> u8 {
        match self.view.playback.volume {
            0 if self.view.playback.song_id.is_none() => 60,
            v => v.min(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u64) -> Song {
        Song { id, track: String::new(), video_id: format!("v{id:010}"), title: format!("S{id}"), author: String::new(), duration_s: 100, by: "a".into() }
    }

    #[test]
    fn notices_a_new_current_song() {
        let mut q = Queue::default();
        assert!(q.update(SongsView::default()), "the first view always counts");
        assert!(!q.update(SongsView::default()));
        let v = SongsView { current: Some(song(1)), queue: vec![song(2)], ..Default::default() };
        assert!(q.update(v.clone()));
        assert!(!q.update(v), "same song, no reload");
        assert_eq!(q.up_next().map(|s| s.id), Some(2));
        let v = SongsView { current: Some(song(2)), ..Default::default() };
        assert!(q.update(v));
    }

    #[test]
    fn reads_the_server_view() {
        let v: SongsView = serde_json::from_str(
            r#"{"settings":{"enabled":true,"who":"followers","maxQueue":50},
                "current":{"id":4,"videoId":"dQw4w9WgXcQ","title":"T","author":"A","durationS":213,"by":"Maple","byId":"1","at":0},
                "queue":[],"playback":{"songId":4,"playing":true,"positionMs":1000,"volume":40,"status":null,"at":5},
                "playerOnline":true,"history":[]}"#,
        )
        .unwrap();
        assert!(v.settings.enabled);
        assert_eq!(v.settings.rest["who"], "followers");
        assert_eq!(v.current.unwrap().duration_s, 213);
        assert_eq!(v.playback.volume, 40);
        let c: SongControl = serde_json::from_str(r#"{"action":"seek","positionMs":5}"#).unwrap();
        assert_eq!(c, SongControl::Seek { position_ms: 5 });
    }
}
