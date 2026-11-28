//! Song requests: viewers queue songs on Sylva with `!sr`, and Sylva Control
//! plays them in the streamer's own Spotify app through Spotify's Web API.
//! No browser, no audio of our own, nothing outside Spotify's official API.
//!
//! Three parts, each usable on its own (no GUI code anywhere):
//! * [`queue`]: the app's copy of Sylva's queue and the server's types.
//! * [`spotify`]: the few Spotify Web API calls the player needs.
//! * [`player`]: one task that ties them together and reports to Sylva.

pub mod player;
pub mod queue;
pub mod spotify;

pub use player::{player, Input, PlayerHandle, PlayerState};
pub use queue::{Playback, Song, SongControl, SongsView};
