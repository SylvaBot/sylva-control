//! Commands the window calls with `invoke`. Every error is returned as a readable string.

use crate::secrets;
use crate::state::{start_realtime, twitch_client, AppState, SeSession};
use pine_core::public::PublicViewers;
use pine_core::se::{AlertAction, Channel, SeClient};
use pine_core::twitch::{self, DeviceCode};
use pine_core::{Activity, Error};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

type CmdResult<T> = Result<T, String>;

/// A Twitch Client ID baked in at build time (`CORTEX_TWITCH_CLIENT_ID`, or the older
/// `PINE_TWITCH_CLIENT_ID`), if any.
const DEFAULT_TWITCH_CLIENT_ID: Option<&str> = match option_env!("CORTEX_TWITCH_CLIENT_ID") {
    Some(id) => Some(id),
    None => option_env!("PINE_TWITCH_CLIENT_ID"),
};

fn se_err(e: Error) -> String {
    if e.is_unauthorized() {
        "StreamElements rejected the token. Copy a fresh JWT from the dashboard and sign in again.".into()
    } else {
        e.to_string()
    }
}

fn twitch_err(e: Error) -> String {
    if e.is_unauthorized() {
        "Twitch rejected the request. Reconnect Twitch, and make sure you approved every permission.".into()
    } else {
        e.to_string()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwitchUser {
    login: String,
    user_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    streamelements: Option<Channel>,
    twitch: Option<TwitchUser>,
    default_twitch_client_id: Option<String>,
    /// A problem found while restoring saved sign-ins, shown once in the UI.
    notice: Option<String>,
}

async fn auth_state(state: &AppState, notice: Option<String>) -> AuthState {
    let streamelements = state.se.read().await.as_ref().map(|s| s.channel.clone());
    let twitch = match state.twitch.read().await.as_ref() {
        Some(c) => {
            let t = c.tokens().await;
            Some(TwitchUser { login: t.login, user_id: t.user_id })
        }
        None => None,
    };
    AuthState {
        streamelements,
        twitch,
        default_twitch_client_id: DEFAULT_TWITCH_CLIENT_ID.map(str::to_string),
        notice,
    }
}

async fn open_se_session(app: &AppHandle, state: &AppState, client: SeClient, channel: Channel) {
    let realtime = start_realtime(app, &client, &channel);
    let old = state.se.write().await.replace(SeSession { client, channel, realtime });
    if let Some(old) = old {
        old.realtime.cancel();
    }
}

/// Called once when the window loads: restores saved sign-ins.
#[tauri::command]
pub async fn restore_session(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AuthState> {
    let mut notice = None;

    if state.se.read().await.is_none() {
        match secrets::load_se_jwt() {
            Ok(Some(jwt)) => match SeClient::new(jwt) {
                Ok(client) => match client.me().await {
                    Ok(channel) => open_se_session(&app, &state, client, channel).await,
                    Err(e) if e.is_unauthorized() => {
                        let _ = secrets::clear_se_jwt();
                        notice = Some("Your saved StreamElements token has expired. Please sign in again.".into());
                    }
                    Err(e) => notice = Some(format!("Could not reach StreamElements: {e}")),
                },
                Err(e) => notice = Some(e.to_string()),
            },
            Ok(None) => {}
            Err(e) => notice = Some(e),
        }
    }

    if state.twitch.read().await.is_none() {
        match secrets::load_twitch() {
            Ok(Some(tokens)) => {
                let client = twitch_client(tokens)?;
                *state.twitch.write().await = Some(client);
            }
            Ok(None) => {}
            Err(e) => notice = notice.or(Some(e)),
        }
    }

    Ok(auth_state(&state, notice).await)
}

/// Validate a StreamElements JWT, save it, and go live.
#[tauri::command]
pub async fn se_sign_in(app: AppHandle, state: State<'_, AppState>, token: String) -> CmdResult<AuthState> {
    let client = SeClient::new(token).map_err(|e| e.to_string())?;
    let channel = client.me().await.map_err(se_err)?;
    secrets::save_se_jwt(client.jwt())?;
    open_se_session(&app, &state, client, channel).await;
    Ok(auth_state(&state, None).await)
}

#[tauri::command]
pub async fn se_sign_out(state: State<'_, AppState>) -> CmdResult<AuthState> {
    if let Some(s) = state.se.write().await.take() {
        s.realtime.cancel();
    }
    secrets::clear_se_jwt()?;
    Ok(auth_state(&state, None).await)
}

async fn with_se<T, F, Fut>(state: &AppState, f: F) -> CmdResult<T>
where
    F: FnOnce(SeClient, String) -> Fut,
    Fut: std::future::Future<Output = Result<T, Error>>,
{
    let (client, channel_id) = {
        let guard = state.se.read().await;
        let s = guard.as_ref().ok_or("Sign in to StreamElements first.")?;
        (s.client.clone(), s.channel.id.clone())
    };
    f(client, channel_id).await.map_err(se_err)
}

/// Recent activity (last 30 days), newest first.
#[tauri::command]
pub async fn get_feed(state: State<'_, AppState>, limit: Option<u32>) -> CmdResult<Vec<Activity>> {
    let limit = limit.unwrap_or(100);
    with_se(&state, |c, ch| async move { c.activities(&ch, limit, 30).await }).await
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    followers: Option<u64>,
    subscribers: Option<u64>,
    /// New followers and subs in the current StreamElements session.
    followers_session: Option<u64>,
    subscribers_session: Option<u64>,
}

/// Follower and subscriber totals from StreamElements.
#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    let t = with_se(&state, |c, ch| async move { c.session_totals(&ch).await }).await?;
    Ok(Stats {
        followers: t.follower_total,
        subscribers: t.subscriber_total,
        followers_session: t.follower_session,
        subscribers_session: t.subscriber_session,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewers {
    /// Where the number came from: "twitch" (signed in to Twitch), "public" (no sign-in needed)
    /// or "none" (no Twitch channel to look up).
    source: &'static str,
    live: bool,
    /// Current viewer count while live.
    viewers: Option<u64>,
}

/// Live viewer count. Uses the Twitch API when Twitch is connected; otherwise looks up the
/// StreamElements channel's Twitch name in a public service, so no Twitch sign-in is needed.
#[tauri::command]
pub async fn get_viewers(state: State<'_, AppState>) -> CmdResult<Viewers> {
    if let Some(client) = state.twitch.read().await.clone() {
        let count = client.viewer_count().await.map_err(twitch_err)?;
        return Ok(Viewers { source: "twitch", live: count.is_some(), viewers: count });
    }
    let login = {
        let guard = state.se.read().await;
        let Some(s) = guard.as_ref() else {
            return Ok(Viewers { source: "none", live: false, viewers: None });
        };
        let twitch = s.channel.provider.as_deref().is_none_or(|p| p.eq_ignore_ascii_case("twitch"));
        match (&s.channel.username, twitch) {
            (Some(name), true) => name.clone(),
            _ => return Ok(Viewers { source: "none", live: false, viewers: None }),
        }
    };
    let public = match state.public_viewers.get() {
        Some(p) => p.clone(),
        None => {
            let p = PublicViewers::new().map_err(|e| e.to_string())?;
            state.public_viewers.get_or_init(|| p).clone()
        }
    };
    let count = public
        .viewer_count(&login)
        .await
        .map_err(|e| format!("Could not get the viewer count: {e}"))?;
    Ok(Viewers { source: "public", live: count.is_some(), viewers: count })
}

async fn twitch_client_or_err(state: &AppState) -> CmdResult<std::sync::Arc<twitch::TwitchClient>> {
    state
        .twitch
        .read()
        .await
        .clone()
        .ok_or_else(|| "Connect Twitch first (Settings, Connect Twitch).".to_string())
}

/// Current stream title and category. `None` when Twitch is not connected.
#[tauri::command]
pub async fn get_channel_info(state: State<'_, AppState>) -> CmdResult<Option<twitch::ChannelInfo>> {
    let Some(client) = state.twitch.read().await.clone() else {
        return Ok(None);
    };
    client.channel_info().await.map(Some).map_err(twitch_err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdStatus {
    /// Twitch did not accept the saved sign-in for ads (it predates the ads permission).
    needs_reconnect: bool,
    schedule: Option<twitch::AdSchedule>,
}

/// The upcoming and the last ad break, for the ad indicator. `schedule` is `None` without Twitch.
#[tauri::command]
pub async fn get_ad_schedule(state: State<'_, AppState>) -> CmdResult<AdStatus> {
    let Some(client) = state.twitch.read().await.clone() else {
        return Ok(AdStatus { needs_reconnect: false, schedule: None });
    };
    match client.ad_schedule().await {
        Ok(s) => Ok(AdStatus { needs_reconnect: false, schedule: Some(s) }),
        Err(e) if e.is_unauthorized() => Ok(AdStatus { needs_reconnect: true, schedule: None }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn search_categories(state: State<'_, AppState>, query: String) -> CmdResult<Vec<twitch::Category>> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let client = twitch_client_or_err(&state).await?;
    client.search_categories(&query).await.map_err(twitch_err)
}

/// Change the stream title and/or category. Needs the `channel:manage:broadcast` permission,
/// so a Twitch connection made before this feature existed has to be reconnected once.
#[tauri::command]
pub async fn update_channel(
    state: State<'_, AppState>,
    title: Option<String>,
    category_id: Option<String>,
) -> CmdResult<twitch::ChannelInfo> {
    if let Some(t) = &title {
        if t.trim().is_empty() {
            return Err("The title cannot be empty.".into());
        }
        if t.chars().count() > 140 {
            return Err("Twitch titles can be at most 140 characters.".into());
        }
    }
    let client = twitch_client_or_err(&state).await?;
    client
        .update_channel(title.as_deref().map(str::trim), category_id.as_deref())
        .await
        .map_err(|e| {
            if e.is_unauthorized() {
                "Twitch did not allow the change. Disconnect and connect Twitch again in Settings so Cortex can ask for permission to edit your title and category.".to_string()
            } else {
                e.to_string()
            }
        })?;
    client.channel_info().await.map_err(twitch_err)
}

#[tauri::command]
pub async fn alert_action(state: State<'_, AppState>, action: AlertAction) -> CmdResult<()> {
    with_se(&state, |c, ch| async move { c.alert_action(&ch, action).await }).await
}

#[tauri::command]
pub async fn reload_overlays(state: State<'_, AppState>) -> CmdResult<()> {
    with_se(&state, |c, ch| async move { c.reload_overlays(&ch).await }).await
}

#[tauri::command]
pub async fn replay_activity(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_se(&state, |c, ch| async move { c.replay(&ch, &id).await }).await
}

/// Start Twitch device sign-in. Returns the code to show; finishes in the background and
/// emits `twitch-auth` with `{ ok: true, login }` or `{ ok: false, error }`.
#[tauri::command]
pub async fn twitch_sign_in(app: AppHandle, state: State<'_, AppState>, client_id: String) -> CmdResult<DeviceCode> {
    let client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        return Err("Enter your Twitch application's Client ID first.".into());
    }
    let http = twitch::http_client().map_err(|e| e.to_string())?;
    let code = twitch::start_device_flow(&http, twitch::ID_BASE, &client_id).await.map_err(|e| {
        if let Error::Http { status: 400, .. } = e {
            "Twitch did not accept that Client ID. Check that the app's Client Type is Public.".to_string()
        } else {
            e.to_string()
        }
    })?;

    let cancel = CancellationToken::new();
    if let Some(old) = state.twitch_login.lock().await.replace(cancel.clone()) {
        old.cancel();
    }

    if let Err(e) = app.opener().open_url(code.verification_uri.clone(), None::<&str>) {
        log::warn!("could not open the browser: {e}");
    }

    let app2 = app.clone();
    let code2 = code.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let result = twitch::finish_device_flow(&http, twitch::ID_BASE, &client_id, &code2, &cancel).await;
        let payload = match result {
            Ok(tokens) => {
                let saved = secrets::save_twitch(&tokens);
                let login = tokens.login.clone();
                match (saved, twitch_client(tokens)) {
                    (Ok(()), Ok(client)) => {
                        *app2.state::<AppState>().twitch.write().await = Some(client);
                        serde_json::json!({ "ok": true, "login": login })
                    }
                    (Err(e), _) | (_, Err(e)) => serde_json::json!({ "ok": false, "error": e }),
                }
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }),
        };
        let _ = app2.emit("twitch-auth", payload);
    });

    Ok(code)
}

#[tauri::command]
pub async fn twitch_cancel_sign_in(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(c) = state.twitch_login.lock().await.take() {
        c.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn twitch_sign_out(state: State<'_, AppState>) -> CmdResult<AuthState> {
    *state.twitch.write().await = None;
    secrets::clear_twitch()?;
    Ok(auth_state(&state, None).await)
}

/// One page of subscribers, followers or chatters from Twitch.
#[tauri::command]
pub async fn get_audience(state: State<'_, AppState>, kind: String, cursor: Option<String>) -> CmdResult<Value> {
    let client = state
        .twitch
        .read()
        .await
        .clone()
        .ok_or("Connect Twitch to see subscribers, followers and chatters.")?;
    let after = cursor.as_deref();
    let page = match kind.as_str() {
        "subscribers" => serde_json::to_value(client.subscribers(after).await.map_err(twitch_err)?),
        "followers" => serde_json::to_value(client.followers(after).await.map_err(twitch_err)?),
        "chatters" => serde_json::to_value(client.chatters(after).await.map_err(twitch_err)?),
        other => return Err(format!("unknown list: {other}")),
    };
    page.map_err(|e| e.to_string())
}

/// Open a help link in the system browser. Only known hosts are allowed.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> CmdResult<()> {
    const ALLOWED: &[&str] = &["streamelements.com", "dev.twitch.tv", "www.twitch.tv", "twitch.tv"];
    let parsed = url::Url::parse(&url).map_err(|_| "invalid link".to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    let ok = parsed.scheme() == "https"
        && ALLOWED.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")));
    if !ok {
        return Err("that link is not allowed".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}
