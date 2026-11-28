use pine_core::public::PublicViewers;
use pine_core::sr::{self, Input, PlayerHandle};
use pine_core::sylva::{self, LiveEvent, SylvaChannel, SylvaClient};
use pine_core::twitch::{TwitchClient, TwitchTokens};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

pub struct SylvaSession {
    pub client: SylvaClient,
    pub channel: SylvaChannel,
    /// Stops this session's live connection and the song player.
    pub live: CancellationToken,
    /// Song requests: plays the queue and reports to Sylva.
    pub player: PlayerHandle,
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

/// Start Sylva's live connection and the song player, and forward both to
/// the window (`activity`, `sylva-status`, `sylva-tips`, `sr`).
pub fn start_sylva(app: &AppHandle, client: &SylvaClient, key: &str, last_seq: u64) -> (CancellationToken, PlayerHandle) {
    let cancel = CancellationToken::new();
    let (player, run) = sr::player(client.clone(), key.to_string(), cancel.clone());
    tauri::async_runtime::spawn(run);
    {
        let app = app.clone();
        let mut watch = player.watch();
        tauri::async_runtime::spawn(async move {
            while watch.changed().await.is_ok() {
                let s = watch.borrow_and_update().clone();
                let _ = app.emit("sr", s);
            }
        });
    }
    let (tx, mut rx) = mpsc::channel::<LiveEvent>(256);
    tauri::async_runtime::spawn(sylva::run_live(client.clone(), key.to_string(), last_seq, tx, cancel.clone()));
    let app = app.clone();
    let to_player = player.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let sent = match ev {
                LiveEvent::Activity(a) => app.emit("activity", *a),
                LiveEvent::Status(s) => app.emit("sylva-status", s),
                LiveEvent::Tips(held) => app.emit("sylva-tips", held),
                LiveEvent::Songs(v) => {
                    to_player.send(Input::Songs(v));
                    Ok(())
                }
                LiveEvent::SongControl(c) => {
                    to_player.send(Input::Control(c));
                    Ok(())
                }
            };
            if let Err(e) = sent {
                log::warn!("could not forward a Sylva event: {e}");
            }
        }
    });
    (cancel, player)
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
