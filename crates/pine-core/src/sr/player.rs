//! The player: ties Sylva's queue to the streamer's Spotify app.
//!
//! It runs as one task. Sylva tells it which song is up (`songs` frames);
//! it starts that track in Spotify, watches Spotify about once a second to
//! know the position and when the track ends, and then asks Sylva for the
//! next song. It reports playback to Sylva on every change and every 10
//! seconds, so the dashboard and the channel page always show what is really
//! playing. A front end only reads [`PlayerState`] and sends [`Input`]s; the
//! UI holds no playback logic.
//!
//! The streamer's own Spotify listening is left alone: nothing is sent to
//! Spotify until play is pressed, and if they put on something else in
//! Spotify the player steps back and waits for play again.

use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::queue::{Playback, Queue, SongControl, SongsView};
use super::spotify::{Spotify, SpotifyError};
use crate::sylva::SylvaClient;

const TICK: Duration = Duration::from_secs(1);
/// Report to Sylva at least this often, in ticks.
const REPORT_TICKS: u32 = 10;
/// Check Spotify this often while paused, in ticks.
const PAUSED_POLL_TICKS: u32 = 5;
/// A track this close to its end counts as finished when Spotify moves on.
const END_SLACK_MS: u64 = 3_000;
/// After this many songs in a row fail to start, stop skipping on our own.
const MAX_AUTO_SKIPS: u32 = 2;

/// What a front end shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub view: SongsView,
    /// The song started in Spotify, by id.
    pub loaded: Option<u64>,
    /// Asking Spotify to start a song.
    pub loading: bool,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    /// A problem worth showing, such as Spotify being closed.
    pub status: Option<String>,
}

#[derive(Debug)]
pub enum Input {
    /// Sylva's latest queue.
    Songs(Box<SongsView>),
    /// A press here or on the dashboard.
    Control(SongControl),
}

#[derive(Clone)]
pub struct PlayerHandle {
    tx: mpsc::UnboundedSender<Input>,
    state: watch::Receiver<PlayerState>,
}

impl PlayerHandle {
    pub fn send(&self, i: Input) {
        let _ = self.tx.send(i);
    }
    pub fn state(&self) -> PlayerState {
        self.state.borrow().clone()
    }
    pub fn watch(&self) -> watch::Receiver<PlayerState> {
        self.state.clone()
    }
}

/// Start the player. Spawn the returned future on the async runtime; it
/// ends when `cancel` fires.
pub fn player(
    client: SylvaClient,
    key: String,
    cancel: CancellationToken,
) -> (PlayerHandle, impl std::future::Future<Output = ()> + Send) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(PlayerState { volume: 60, ..Default::default() });
    let run = Player::new(client, key, state_tx).run(rx, cancel);
    (PlayerHandle { tx, state: state_rx }, run)
}

struct Player {
    client: SylvaClient,
    key: String,
    spotify: Spotify,
    queue: Queue,
    state: PlayerState,
    out: watch::Sender<PlayerState>,
    /// Play as soon as a song is up. Off at launch until the streamer
    /// presses play, then on until they pause.
    wants_play: bool,
    /// The Spotify track the song up now was started as.
    started: Option<String>,
    failures: u32,
    ticks: u32,
    last_report: Option<Playback>,
}

impl Player {
    fn new(client: SylvaClient, key: String, out: watch::Sender<PlayerState>) -> Self {
        Player {
            spotify: Spotify::new(client.clone(), key.clone()),
            client,
            key,
            queue: Queue::default(),
            state: PlayerState { volume: 60, ..Default::default() },
            out,
            wants_play: false,
            started: None,
            failures: 0,
            ticks: 0,
            last_report: None,
        }
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Input>, cancel: CancellationToken) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        if let Ok(v) = self.client.songs(&self.key).await {
            self.take_view(v).await;
        }
        loop {
            let mut heartbeat = false;
            tokio::select! {
                _ = cancel.cancelled() => return,
                Some(input) = rx.recv() => match input {
                    Input::Songs(v) => self.take_view(*v).await,
                    Input::Control(c) => self.control(c).await,
                },
                _ = tick.tick() => {
                    self.ticks = self.ticks.wrapping_add(1);
                    let poll = self.started.is_some()
                        && (self.state.playing || self.ticks.is_multiple_of(PAUSED_POLL_TICKS));
                    if poll {
                        self.poll().await;
                    }
                    heartbeat = self.ticks.is_multiple_of(REPORT_TICKS);
                }
            }
            self.publish();
            self.report(heartbeat).await;
        }
    }

    fn publish(&mut self) {
        self.state.view = self.queue.view().clone();
        self.out.send_if_modified(|s| {
            if *s == self.state {
                false
            } else {
                *s = self.state.clone();
                true
            }
        });
    }

    /// Tell Sylva what is playing: on any change, or on the heartbeat.
    async fn report(&mut self, heartbeat: bool) {
        let p = Playback {
            song_id: self.state.loaded,
            playing: self.state.playing,
            position_ms: self.state.position_ms,
            volume: self.state.volume,
            status: self.state.status.clone().or_else(|| self.state.loading.then(|| "Starting it in Spotify".into())),
        };
        let changed = match &self.last_report {
            None => true,
            Some(old) => {
                old.song_id != p.song_id || old.playing != p.playing || old.volume != p.volume || old.status != p.status
            }
        };
        if !(heartbeat || changed) {
            return;
        }
        if let Err(e) = self.client.report_playback(&self.key, &p).await {
            log::warn!("could not report playback: {e}");
        }
        self.last_report = Some(p);
    }

    async fn take_view(&mut self, v: SongsView) {
        let first = !self.queue.known();
        let changed = self.queue.update(v);
        if first {
            self.state.volume = self.queue.last_volume();
        }
        if changed {
            self.load_current().await;
        }
    }

    /// A new song is up: start it in Spotify if the streamer pressed play.
    async fn load_current(&mut self) {
        let was_ours = self.started.take().is_some();
        self.state.loaded = None;
        self.state.position_ms = 0;
        self.state.playing = false;
        self.state.status = None;
        self.state.loading = false;
        let Some(song) = self.queue.current().cloned() else {
            self.state.duration_ms = 0;
            // The queue ran out: stop Spotify moving on to songs nobody asked for.
            if was_ours && self.wants_play {
                let _ = self.spotify.pause().await;
            }
            return;
        };
        self.state.duration_ms = song.duration_s as u64 * 1000;
        if self.wants_play {
            self.start().await;
        }
    }

    /// Start the song up now in Spotify, where the last session left it.
    async fn start(&mut self) {
        let Some(song) = self.queue.current().cloned() else { return };
        if song.track.is_empty() {
            self.fail("This song was asked for on YouTube before Spotify, so it can't play".into()).await;
            return;
        }
        let p = &self.queue.view().playback;
        let from = if p.song_id == Some(song.id) { p.position_ms } else { 0 };
        self.state.loading = true;
        self.publish();
        let r = self.spotify.play(&song.track, from).await;
        self.state.loading = false;
        match r {
            Ok(()) => {
                self.started = Some(song.track.clone());
                self.state.loaded = Some(song.id);
                self.state.playing = true;
                self.state.position_ms = from;
                self.state.status = None;
                self.failures = 0;
            }
            Err(e @ SpotifyError::Other(_)) => self.fail(format!("Could not start this song: {e}")).await,
            // Spotify closed, no Premium, not connected: skipping would not help.
            Err(e) => {
                self.state.playing = false;
                self.state.status = Some(e.to_string());
            }
        }
    }

    /// A song that will not start: skip it, unless several in a row failed.
    async fn fail(&mut self, message: String) {
        log::warn!("{message}");
        self.failures += 1;
        self.state.playing = false;
        if self.failures <= MAX_AUTO_SKIPS {
            self.state.status = Some(format!("{message}. Skipping it."));
            self.publish();
            self.report(false).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            Box::pin(self.next()).await;
        } else {
            self.state.status = Some(format!("{message}. Press skip to try the next song."));
        }
    }

    async fn next(&mut self) {
        let from = self.queue.current().map(|s| s.id);
        // The answer and the `songs` frame both carry the new queue; take the
        // answer now so the next song starts without waiting for the frame.
        match self.client.songs_action(&self.key, "next", serde_json::json!({ "from": from })).await {
            Ok(v) => Box::pin(self.take_view(v)).await,
            Err(e) => log::warn!("could not move to the next song: {e}"),
        }
    }

    /// Ask Spotify what it is doing, and move on when our track ended.
    async fn poll(&mut self) {
        let Some(ours) = self.started.clone() else { return };
        let now = match self.spotify.now().await {
            Ok(n) => n,
            Err(e @ (SpotifyError::NotConnected(_) | SpotifyError::Premium)) => {
                self.state.status = Some(e.to_string());
                return;
            }
            Err(e) => {
                log::debug!("spotify poll: {e}");
                return;
            }
        };
        let last = self.state.position_ms;
        let total = self.state.duration_ms;
        let near_end = total > 0 && last + END_SLACK_MS >= total;
        if let Some(v) = now.volume {
            self.state.volume = v;
        }
        if now.track.as_deref() == Some(ours.as_str()) {
            if now.duration_ms > 0 {
                self.state.duration_ms = now.duration_ms;
            }
            // A one track play stops at the end and rewinds to the start.
            let at_end = now.position_ms == 0 || now.position_ms + 1_500 >= now.duration_ms;
            if !now.playing && near_end && at_end {
                self.ended().await;
                return;
            }
            self.state.position_ms = now.position_ms;
            self.state.playing = now.playing;
            return;
        }
        if near_end {
            // Spotify finished it and moved on by itself (autoplay).
            self.ended().await;
            return;
        }
        // The streamer put something else on in Spotify: step back.
        self.started = None;
        self.state.loaded = None;
        self.state.playing = false;
        self.wants_play = false;
        self.state.status = Some("Spotify is playing something else. Press play to go back to the requests".into());
    }

    async fn ended(&mut self) {
        self.state.playing = false;
        self.state.position_ms = self.state.duration_ms;
        self.next().await;
    }

    async fn control(&mut self, c: SongControl) {
        let ours = self.started.is_some() && self.state.loaded == self.queue.current().map(|s| s.id);
        let r = match c {
            SongControl::Play => {
                self.wants_play = true;
                if ours {
                    let r = self.spotify.resume().await;
                    if r.is_ok() {
                        self.state.playing = true;
                        self.state.status = None;
                    }
                    r
                } else {
                    self.failures = 0;
                    self.start().await;
                    Ok(())
                }
            }
            SongControl::Pause => {
                self.wants_play = false;
                self.state.playing = false;
                if ours {
                    self.spotify.pause().await
                } else {
                    Ok(())
                }
            }
            SongControl::Seek { position_ms } => {
                if !ours {
                    return;
                }
                let to = if self.state.duration_ms > 0 { position_ms.min(self.state.duration_ms) } else { position_ms };
                self.state.position_ms = to;
                // A seek is news for everyone showing the position.
                self.last_report = None;
                self.spotify.seek(to).await
            }
            SongControl::Volume { volume } => {
                self.state.volume = volume.min(100);
                self.spotify.volume(self.state.volume).await
            }
        };
        if let Err(e) = r {
            self.state.status = Some(e.to_string());
        }
    }
}
