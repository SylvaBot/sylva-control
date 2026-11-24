use pine_core::se::astro::{self, AstroConfig, AstroEvent};
use pine_core::se::{Channel, SeClient};
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

#[derive(Default)]
pub struct AppState {
    pub se: RwLock<Option<SeSession>>,
    pub twitch: RwLock<Option<Arc<TwitchClient>>>,
    /// Cancels an in-progress Twitch device sign-in.
    pub twitch_login: Mutex<Option<CancellationToken>>,
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
                AstroEvent::Activity(a) => app.emit("activity", *a),
                AstroEvent::Status(s) => app.emit("realtime-status", s),
            };
            if let Err(e) = sent {
                log::warn!("could not forward realtime event: {e}");
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
