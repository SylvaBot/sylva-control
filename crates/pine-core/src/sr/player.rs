//! The player: ties the queue, the resolver and the audio engine together.
//!
//! It runs as one task. Sylva tells it which song is up (`songs` frames);
//! it fetches that song's audio, plays it, fetches the next one ahead, and
//! asks Sylva for the next song when one ends. It reports playback to Sylva
//! on every change and every 10 seconds, so the dashboard and the channel
//! page always show what is really playing. A front end only reads
//! [`PlayerState`] and sends [`Input`]s; the UI holds no playback logic.

use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::audio::{AudioEngine, Command, Event};
use super::queue::{Playback, Queue, SongControl, SongsView};
use super::resolve;
use crate::sylva::SylvaClient;

const REPORT_EVERY: Duration = Duration::from_secs(10);
/// After this many songs in a row fail to load, stop skipping on our own:
/// YouTube is likely refusing every song, and skipping would empty the queue.
const MAX_AUTO_SKIPS: u32 = 2;

/// What a front end shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub view: SongsView,
    /// The song the engine has loaded, by id.
    pub loaded: Option<u64>,
    pub loading: bool,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    /// A problem worth showing, such as a song that would not load.
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

/// A fetched song, or why it failed.
struct Fetched {
    video_id: String,
    ahead: bool,
    result: Result<Vec<u8>, String>,
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
    http: reqwest::Client,
    queue: Queue,
    state: PlayerState,
    out: watch::Sender<PlayerState>,
    /// Play as soon as a song is ready. Off at launch until the streamer
    /// presses play, then on until they pause.
    wants_play: bool,
    /// The fetch for the song up now, so a skip can stop it.
    fetching: Option<(String, CancellationToken)>,
    /// The next song's audio, fetched ahead.
    ahead: Option<(String, Vec<u8>)>,
    ahead_fetching: Option<String>,
    failures: u32,
    last_report: Option<Playback>,
    volume_known: bool,
}

impl Player {
    fn new(client: SylvaClient, key: String, out: watch::Sender<PlayerState>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0")
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Player {
            client,
            key,
            http,
            queue: Queue::default(),
            state: PlayerState { volume: 60, ..Default::default() },
            out,
            wants_play: false,
            fetching: None,
            ahead: None,
            ahead_fetching: None,
            failures: 0,
            last_report: None,
            volume_known: false,
        }
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Input>, cancel: CancellationToken) {
        let (ev_tx, mut events) = mpsc::unbounded_channel();
        let engine = AudioEngine::start(ev_tx);
        engine.send(Command::Volume(self.state.volume));
        let (fetch_tx, mut fetched) = mpsc::unbounded_channel::<Fetched>();
        let mut tick = tokio::time::interval(REPORT_EVERY);

        if let Ok(v) = self.client.songs(&self.key).await {
            self.take_view(v, &engine, &fetch_tx);
        }
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    engine.send(Command::Stop);
                    if let Some((_, c)) = self.fetching.take() { c.cancel(); }
                    return;
                }
                Some(input) = rx.recv() => match input {
                    Input::Songs(v) => self.take_view(*v, &engine, &fetch_tx),
                    Input::Control(c) => self.control(c, &engine),
                },
                Some(ev) = events.recv() => self.engine_event(ev, &engine).await,
                Some(f) = fetched.recv() => self.fetched(f, &engine).await,
                _ = tick.tick() => self.report(true).await,
            }
            self.publish();
            self.report(false).await;
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
            status: self.state.status.clone().or_else(|| self.state.loading.then(|| "Loading the song".into())),
        };
        let changed = match &self.last_report {
            None => true,
            Some(old) => {
                old.song_id != p.song_id
                    || old.playing != p.playing
                    || old.volume != p.volume
                    || old.status != p.status
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

    fn take_view(&mut self, v: SongsView, engine: &AudioEngine, fetch_tx: &mpsc::UnboundedSender<Fetched>) {
        let changed = self.queue.update(v);
        if !self.volume_known {
            self.volume_known = true;
            self.state.volume = self.queue.last_volume();
            engine.send(Command::Volume(self.state.volume));
        }
        if changed {
            self.load_current(engine, fetch_tx);
        }
        self.fetch_ahead(fetch_tx);
    }

    fn load_current(&mut self, engine: &AudioEngine, fetch_tx: &mpsc::UnboundedSender<Fetched>) {
        if let Some((_, c)) = self.fetching.take() {
            c.cancel();
        }
        self.state.loaded = None;
        self.state.position_ms = 0;
        self.state.duration_ms = 0;
        self.state.playing = false;
        self.state.status = None;
        let Some(song) = self.queue.current().cloned() else {
            self.state.loading = false;
            engine.send(Command::Stop);
            return;
        };
        if let Some((id, bytes)) = self.ahead.take() {
            if id == song.video_id {
                self.state.loading = true;
                let _ = fetch_tx.send(Fetched { video_id: id, ahead: false, result: Ok(bytes) });
                return;
            }
        }
        self.state.loading = true;
        let cancel = CancellationToken::new();
        self.fetching = Some((song.video_id.clone(), cancel.clone()));
        spawn_fetch(self.http.clone(), song.video_id, false, cancel, fetch_tx.clone());
    }

    fn fetch_ahead(&mut self, fetch_tx: &mpsc::UnboundedSender<Fetched>) {
        let Some(next) = self.queue.up_next() else { return };
        let have = self.ahead.as_ref().is_some_and(|(id, _)| *id == next.video_id);
        if have || self.ahead_fetching.as_deref() == Some(next.video_id.as_str()) {
            return;
        }
        // One download at a time keeps the song up now quick to start.
        if self.state.loading {
            return;
        }
        self.ahead_fetching = Some(next.video_id.clone());
        spawn_fetch(self.http.clone(), next.video_id.clone(), true, CancellationToken::new(), fetch_tx.clone());
    }

    async fn fetched(&mut self, f: Fetched, engine: &AudioEngine) {
        if f.ahead {
            self.ahead_fetching = None;
            if let Ok(bytes) = f.result {
                self.ahead = Some((f.video_id, bytes));
            }
            return;
        }
        let Some(song) = self.queue.current().cloned() else { return };
        if song.video_id != f.video_id {
            return;
        }
        self.fetching = None;
        match f.result {
            Ok(bytes) => {
                // Pick up where the last session left off.
                let p = &self.queue.view().playback;
                let start_ms = if p.song_id == Some(song.id) { p.position_ms } else { 0 };
                self.state.loaded = Some(song.id);
                self.state.position_ms = start_ms;
                engine.send(Command::Load { bytes, start_ms, play: self.wants_play });
            }
            Err(e) => {
                self.state.loading = false;
                self.fail(format!("Could not load this song: {e}")).await;
            }
        }
    }

    /// A song that will not play: skip it, unless several in a row failed.
    async fn fail(&mut self, message: String) {
        log::warn!("{message}");
        self.failures += 1;
        self.state.playing = false;
        if self.failures <= MAX_AUTO_SKIPS {
            self.state.status = Some(format!("{message}. Skipping it."));
            self.report(false).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            self.next().await;
        } else {
            self.state.status = Some(format!("{message}. Press skip to try the next song."));
        }
    }

    async fn next(&mut self) {
        let from = self.queue.current().map(|s| s.id);
        match self.client.songs_action(&self.key, "next", serde_json::json!({ "from": from })).await {
            // The answer and the `songs` frame both carry the new queue.
            Ok(v) => {
                let _ = v;
            }
            Err(e) => log::warn!("could not move to the next song: {e}"),
        }
    }

    async fn engine_event(&mut self, ev: Event, engine: &AudioEngine) {
        match ev {
            Event::Loaded { duration_ms } => {
                self.state.loading = false;
                self.state.duration_ms = duration_ms;
                self.state.playing = self.wants_play;
                self.failures = 0;
            }
            Event::Position { ms, playing } => {
                self.state.position_ms = ms;
                self.state.playing = playing;
            }
            Event::Ended => {
                self.state.playing = false;
                self.next().await;
            }
            Event::Failed(m) if m.starts_with("no sound device") => {
                self.state.status = Some(format!("Sylva Control has {m}"));
            }
            Event::Failed(m) => {
                self.state.loading = false;
                engine.send(Command::Stop);
                self.state.loaded = None;
                self.fail(format!("Could not play this song: {m}")).await;
            }
        }
    }

    fn control(&mut self, c: SongControl, engine: &AudioEngine) {
        match c {
            SongControl::Play => {
                self.wants_play = true;
                if self.state.loaded.is_some() {
                    engine.send(Command::Play);
                    self.state.playing = true;
                }
                if self.state.status.as_deref().is_some_and(|s| s.contains("No sound") || s.contains("sound device")) {
                    self.state.status = None;
                }
            }
            SongControl::Pause => {
                self.wants_play = false;
                engine.send(Command::Pause);
                self.state.playing = false;
            }
            SongControl::Seek { position_ms } => {
                let to = if self.state.duration_ms > 0 { position_ms.min(self.state.duration_ms) } else { position_ms };
                engine.send(Command::Seek(to));
                self.state.position_ms = to;
                // A seek is news for everyone showing the position.
                self.last_report = None;
            }
            SongControl::Volume { volume } => {
                self.state.volume = volume.min(100);
                engine.send(Command::Volume(self.state.volume));
            }
        }
    }
}

fn spawn_fetch(
    http: reqwest::Client,
    video_id: String,
    ahead: bool,
    cancel: CancellationToken,
    tx: mpsc::UnboundedSender<Fetched>,
) {
    tokio::spawn(async move {
        let result = async {
            resolve::fetch_audio(&http, &video_id, &cancel).await.map_err(|e| e.to_string())
        }
        .await;
        if !cancel.is_cancelled() {
            let _ = tx.send(Fetched { video_id, ahead, result });
        }
    });
}
