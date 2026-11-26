use pine_core::public::PublicViewers;
use pine_core::se::astro::{self, AstroConfig, AstroEvent};
use pine_core::se::{Channel, SeClient};
use pine_core::sylva::{self, LiveEvent, SylvaChannel, SylvaClient};
use pine_core::twitch::{TwitchClient, TwitchTokens};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

pub struct SeSession {
    pub client: SeClient,
    pub channel: Channel,
    /// Stops this session's realtime connection.
    pub realtime: CancellationToken,
}

pub struct SylvaSession {
    pub client: SylvaClient,
    pub channel: SylvaChannel,
    /// Stops this session's live connection.
    pub live: CancellationToken,
}

#[derive(Default)]
pub struct AppState {
    /// Sylva, when connected: the activity feed then comes from there, the
    /// same feed every Sylva page shows.
    pub sylva: RwLock<Option<SylvaSession>>,
    /// Cancels an in-progress Sylva device sign-in.
    pub sylva_login: Mutex<Option<CancellationToken>>,
    pub se: RwLock<Option<SeSession>>,
    pub twitch: RwLock<Option<Arc<TwitchClient>>>,
    /// Cancels an in-progress Twitch device sign-in.
    pub twitch_login: Mutex<Option<CancellationToken>>,
    /// Viewer count without a Twitch sign-in, created on first use.
    pub public_viewers: std::sync::OnceLock<PublicViewers>,
}

/// Start the Astro connection for a session and forward its events to the window.
pub fn start_realtime(app: &AppHandle, client: &SeClient, channel: &Channel) -> CancellationToken {
    let cancel = CancellationToken::new();
    let (tx, mut rx) = mpsc::channel::<AstroEvent>(256);
    let cfg = AstroConfig::new(client.jwt(), channel.id.clone());
    tauri::async_runtime::spawn(astro::run(cfg, tx, cancel.clone()));

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let sent = match ev {
                AstroEvent::Activity(a) => {
                    // With Sylva connected the feed is Sylva's alone.
                    use tauri::Manager;
                    if app.state::<AppState>().sylva.read().await.is_some() {
                        continue;
                    }
                    app.emit("activity", *a)
                }
                AstroEvent::Status(s) => app.emit("realtime-status", s),
            };
            if let Err(e) = sent {
                log::warn!("could not forward realtime event: {e}");
            }
        }
    });
    cancel
}

/// Start Sylva's live connection and forward its activity to the window.
pub fn start_sylva(app: &AppHandle, client: &SylvaClient, key: &str, last_seq: u64) -> CancellationToken {
    let cancel = CancellationToken::new();
    let (tx, mut rx) = mpsc::channel::<LiveEvent>(256);
    tauri::async_runtime::spawn(sylva::run_live(client.clone(), key.to_string(), last_seq, tx, cancel.clone()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let sent = match ev {
                LiveEvent::Activity(a) => app.emit("activity", *a),
                LiveEvent::Status(s) => app.emit("sylva-status", s),
            };
            if let Err(e) = sent {
                log::warn!("could not forward a Sylva event: {e}");
            }
        }
    });
    cancel
}

/// Build a Twitch client that writes refreshed tokens back to the credential store.
pub fn twitch_client(tokens: TwitchTokens) -> Result<Arc<TwitchClient>, String> {
    TwitchClient::new(
        tokens,
        Some(Box::new(|t: &TwitchTokens| {
            if let Err(e) = crate::secrets::save_twitch(t) {
                log::warn!("{e}");
            }
        })),
    )
    .map_err(|e| e.to_string())
}
