//! Song requests: viewers queue YouTube songs on Sylva with `!sr`, and
//! Sylva Control plays their audio. No video, no browser, no other player.
//!
//! Four parts, each usable on its own (no GUI code anywhere):
//! * [`queue`]: the app's copy of Sylva's queue and the server's types.
//! * [`resolve`]: a YouTube video id to its audio bytes.
//! * [`audio`]: an engine thread that decodes and plays those bytes.
//! * [`player`]: one task that ties them together and reports to Sylva.

pub mod audio;
pub mod player;
pub mod queue;
pub mod resolve;

pub use player::{player, Input, PlayerHandle, PlayerState};
pub use queue::{Playback, Song, SongControl, SongsView};
