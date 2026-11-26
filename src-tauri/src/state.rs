use pine_core::public::PublicViewers;
use pine_core::sylva::{self, LiveEvent, SylvaChannel, SylvaClient};
use pine_core::twitch::{TwitchClient, TwitchTokens};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

pub struct SylvaSession {
    pub client: SylvaClient,
    pub channel: SylvaChannel,
    /// Stops this session's live connection.
    pub live: CancellationToken,
}

#[derive(Default)]
pub struct AppState {
    /// Sylva: activity, alerts and overlays all come from here.
    pub sylva: RwLock<Option<SylvaSession>>,
    /// Cancels an in-progress Sylva device sign-in.
    pub sylva_login: Mutex<Option<CancellationToken>>,
    pub twitch: RwLock<Option<Arc<TwitchClient>>>,
    /// Cancels an in-progress Twitch device sign-in.
    pub twitch_login: Mutex<Option<CancellationToken>>,
    /// Viewer count without a Twitch sign-in, created on first use.
    pub public_viewers: std::sync::OnceLock<PublicViewers>,
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
