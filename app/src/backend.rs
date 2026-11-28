//! Everything the window asks for, run on the tokio runtime. Results come back to the window
//! as `Ev` values (each one also asks for a redraw), so the window never waits on the network.
//!
//! This is the same work the Tauri commands did before Sylva Control became a native app.

use crate::secrets;
use pine_core::public::{PublicViewers, SiteViewers};
use pine_core::sr::{self, Input, PlayerHandle, PlayerState, SongControl};
use pine_core::sylva::{self, AlertState, LiveEvent, LiveStatus, SylvaChannel, SylvaClient};
use pine_core::twitch::{self, AdSchedule, Category, ChannelInfo, DeviceCode, TwitchClient, TwitchTokens};
use pine_core::{Activity, Error};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// A Twitch Client ID baked in at build time, if any.
const DEFAULT_TWITCH_CLIENT_ID: Option<&str> = match option_env!("CORTEX_TWITCH_CLIENT_ID") {
    Some(id) => Some(id),
    None => option_env!("PINE_TWITCH_CLIENT_ID"),
};

pub const SYLVA_GONE: &str = "Sylva no longer accepts Sylva Control. Sign in again.";

fn sylva_err(e: Error) -> String {
    if e.is_unauthorized() {
        SYLVA_GONE.into()
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

#[derive(Clone, Default)]
pub struct AuthState {
    pub sylva: Option<SylvaChannel>,
    /// Twitch login, when connected.
    pub twitch: Option<String>,
    pub default_twitch_client_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ViewerSource {
    /// The feed twitch.tv itself reads. Fastest, unofficial.
    Site,
    Twitch,
    Public,
    None,
}

pub struct Viewers {
    pub source: ViewerSource,
    pub live: bool,
    pub viewers: Option<u64>,
}

pub struct AdStatus {
    pub needs_reconnect: bool,
    pub schedule: Option<AdSchedule>,
}

pub struct UpdateInfo {
    pub supported: bool,
    pub behind: u32,
    pub changes: Vec<String>,
}

/// What background work tells the window.
pub enum Ev {
    Auth { auth: AuthState, notice: Option<String> },
    Feed(Result<Vec<Activity>, String>),
    Live(Box<Activity>),
    TipsChanged,
    HeldTips(Vec<Value>),
    Stats { followers: Option<u64>, subscribers: Option<u64> },
    Viewers(Result<Viewers, String>),
    ChannelInfo { info: Result<Option<ChannelInfo>, String>, quiet: bool },
    ChannelSaved(Result<ChannelInfo, String>),
    Ads(Result<AdStatus, String>),
    Categories { req: u64, result: Result<Vec<Category>, String> },
    AlertState(AlertState),
    SylvaCode(String),
    SylvaSignIn(Result<Option<String>, String>),
    TwitchCode(DeviceCode),
    TwitchSignIn(Result<String, String>),
    Sr(Option<PlayerState>),
    SylvaUnauthorized,
    /// A button's work finished: its key, and the error if any.
    Done { key: String, result: Result<(), String>, ok_text: Option<String> },
    Update(Result<UpdateInfo, String>, bool),
    UpdateStatus { step: &'static str, text: String },
}

pub type Notify = Arc<dyn Fn(Ev) + Send + Sync>;

struct SylvaSession {
    client: SylvaClient,
    channel: SylvaChannel,
    live: CancellationToken,
    player: PlayerHandle,
}

pub struct Backend {
    pub notify: Notify,
    sylva: RwLock<Option<SylvaSession>>,
    sylva_login: Mutex<Option<CancellationToken>>,
    twitch: RwLock<Option<Arc<TwitchClient>>>,
    twitch_login: Mutex<Option<CancellationToken>>,
    public_viewers: std::sync::OnceLock<PublicViewers>,
    site_viewers: std::sync::OnceLock<SiteViewers>,
}

impl Backend {
    pub fn new(notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            notify,
            sylva: RwLock::new(None),
            sylva_login: Mutex::new(None),
            twitch: RwLock::new(None),
            twitch_login: Mutex::new(None),
            public_viewers: std::sync::OnceLock::new(),
            site_viewers: std::sync::OnceLock::new(),
        })
    }

    fn send(&self, e: Ev) {
        (self.notify)(e)
    }

    async fn auth(&self) -> AuthState {
        let sylva = self.sylva.read().await.as_ref().map(|s| s.channel.clone());
        let twitch = match self.twitch.read().await.as_ref() {
            Some(c) => Some(c.tokens().await.login),
            None => None,
        };
        AuthState { sylva, twitch, default_twitch_client_id: DEFAULT_TWITCH_CLIENT_ID.map(str::to_string) }
    }

    async fn send_auth(&self, notice: Option<String>) {
        let auth = self.auth().await;
        self.send(Ev::Auth { auth, notice });
    }

    /// Check a Sylva token, find the channel and start the live feed and the song player.
    async fn open_sylva(self: &Arc<Self>, client: SylvaClient) -> Result<(), Error> {
        let channel = client.channel().await?;
        let (_, head) = client.history(&channel.key, 1).await?;
        let cancel = CancellationToken::new();
        let (player, run) = sr::player(client.clone(), channel.key.clone(), cancel.clone());
        tokio::spawn(run);
        {
            let notify = self.notify.clone();
            let mut watch = player.watch();
            let c = cancel.clone();
            notify(Ev::Sr(Some(watch.borrow().clone())));
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = c.cancelled() => return,
                        r = watch.changed() => if r.is_err() { return },
                    }
                    let s = watch.borrow_and_update().clone();
                    notify(Ev::Sr(Some(s)));
                }
            });
        }
        let (tx, mut rx) = mpsc::channel::<LiveEvent>(256);
        tokio::spawn(sylva::run_live(client.clone(), channel.key.clone(), head, tx, cancel.clone()));
        {
            let notify = self.notify.clone();
            let to_player = player.clone();
            tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        LiveEvent::Activity(a) => notify(Ev::Live(a)),
                        LiveEvent::Status(LiveStatus::Unauthorized) => notify(Ev::SylvaUnauthorized),
                        LiveEvent::Status(_) => {}
                        LiveEvent::Tips(_) => notify(Ev::TipsChanged),
                        LiveEvent::Songs(v) => to_player.send(Input::Songs(v)),
                        LiveEvent::SongControl(c) => to_player.send(Input::Control(c)),
                    }
                }
            });
        }
        let old = self.sylva.write().await.replace(SylvaSession { client, channel, live: cancel, player });
        if let Some(old) = old {
            old.live.cancel();
        }
        Ok(())
    }

    /// Restores saved sign-ins. Called once at start.
    pub async fn restore(self: Arc<Self>) {
        let mut notice = None;
        let _ = secrets::clear_se_jwt();
        if self.sylva.read().await.is_none() {
            match secrets::load_sylva() {
                Ok(Some(token)) => match SylvaClient::new(token) {
                    Ok(client) => match self.open_sylva(client).await {
                        Ok(()) => {}
                        Err(e) if e.is_unauthorized() => {
                            let _ = secrets::clear_sylva();
                            notice = Some(SYLVA_GONE.to_string());
                        }
                        Err(e) => notice = Some(format!("Could not reach Sylva: {e}")),
                    },
                    Err(e) => notice = Some(e.to_string()),
                },
                Ok(None) => {}
                Err(e) => notice = Some(e),
            }
        }
        if self.twitch.read().await.is_none() {
            match secrets::load_twitch() {
                Ok(Some(tokens)) => match twitch_client(tokens) {
                    Ok(c) => *self.twitch.write().await = Some(c),
                    Err(e) => notice = notice.or(Some(e)),
                },
                Ok(None) => {}
                Err(e) => notice = notice.or(Some(e)),
            }
        }
        self.send_auth(notice).await;
    }

    pub async fn sylva_sign_in(self: Arc<Self>) {
        let http = match sylva::http_client() {
            Ok(h) => h,
            Err(e) => return self.send(Ev::SylvaSignIn(Err(e.to_string()))),
        };
        let code = match sylva::start_device_flow(&http, sylva::BASE).await {
            Ok(c) => c,
            Err(e) => return self.send(Ev::SylvaSignIn(Err(format!("Could not start sign-in: {e}")))),
        };
        let cancel = CancellationToken::new();
        if let Some(old) = self.sylva_login.lock().await.replace(cancel.clone()) {
            old.cancel();
        }
        if let Err(e) = open::that_detached(&code.verification_uri_complete) {
            log::warn!("could not open the browser: {e}");
        }
        self.send(Ev::SylvaCode(code.user_code.clone()));
        let result = match sylva::finish_device_flow(&http, sylva::BASE, &code, &cancel).await {
            Ok(token) => match (secrets::save_sylva(&token), SylvaClient::new(token)) {
                (Ok(()), Ok(client)) => match self.open_sylva(client).await {
                    Ok(()) => Ok(self.sylva.read().await.as_ref().and_then(|s| s.channel.name.clone())),
                    Err(e) => Err(sylva_err(e)),
                },
                (Err(e), _) => Err(e),
                (_, Err(e)) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        };
        let ok = result.is_ok();
        self.send(Ev::SylvaSignIn(result));
        if ok {
            self.send_auth(None).await;
        }
    }

    pub async fn sylva_sign_out(self: Arc<Self>) {
        if let Some(s) = self.sylva.write().await.take() {
            s.live.cancel();
        }
        self.send(Ev::Sr(None));
        let notice = secrets::clear_sylva().err();
        self.send_auth(notice).await;
    }

    async fn with_sylva(&self) -> Result<(SylvaClient, String), String> {
        let guard = self.sylva.read().await;
        let s = guard.as_ref().ok_or("Sign in to Sylva first.")?;
        Ok((s.client.clone(), s.channel.key.clone()))
    }

    pub async fn feed(self: Arc<Self>) {
        let r = match self.with_sylva().await {
            Ok((c, key)) => c.history(&key, 100).await.map(|(a, _)| a).map_err(sylva_err),
            Err(e) => Err(e),
        };
        self.send(Ev::Feed(r));
    }

    pub async fn stats(self: Arc<Self>) {
        let Some(client) = self.twitch.read().await.clone() else {
            return self.send(Ev::Stats { followers: None, subscribers: None });
        };
        let followers = client.followers(None).await.ok().and_then(|p| p.total);
        let subscribers = client.subscribers(None).await.ok().and_then(|p| p.total);
        self.send(Ev::Stats { followers, subscribers });
    }

    pub async fn viewers(self: Arc<Self>) {
        let r = self.viewers_inner().await;
        self.send(Ev::Viewers(r));
    }

    async fn viewers_inner(&self) -> Result<Viewers, String> {
        // Try the feed twitch.tv reads first: it is fresher than Helix and DecAPI. It is not an
        // official API, so any failure quietly falls through to the official paths below.
        let channel = match self.twitch.read().await.clone() {
            Some(c) => Some(c.tokens().await.login),
            None => match self.sylva.read().await.as_ref() {
                Some(s) if s.channel.key.starts_with("twitch:") => s.channel.login.clone(),
                _ => None,
            },
        };
        if let Some(login) = channel {
            let site = match self.site_viewers.get() {
                Some(p) => Some(p.clone()),
                None => SiteViewers::new().ok().map(|p| self.site_viewers.get_or_init(|| p).clone()),
            };
            if let Some(site) = site {
                if let Ok(count) = site.viewer_count(&login).await {
                    return Ok(Viewers { source: ViewerSource::Site, live: count.is_some(), viewers: count });
                }
            }
        }
        if let Some(client) = self.twitch.read().await.clone() {
            let count = client.viewer_count().await.map_err(twitch_err)?;
            return Ok(Viewers { source: ViewerSource::Twitch, live: count.is_some(), viewers: count });
        }
        let login = {
            let guard = self.sylva.read().await;
            match guard.as_ref() {
                Some(s) if s.channel.key.starts_with("twitch:") && s.channel.login.is_some() => s.channel.login.clone().unwrap(),
                _ => return Ok(Viewers { source: ViewerSource::None, live: false, viewers: None }),
            }
        };
        let public = match self.public_viewers.get() {
            Some(p) => p.clone(),
            None => {
                let p = PublicViewers::new().map_err(|e| e.to_string())?;
                self.public_viewers.get_or_init(|| p).clone()
            }
        };
        let count = public.viewer_count(&login).await.map_err(|e| format!("Could not get the viewer count: {e}"))?;
        Ok(Viewers { source: ViewerSource::Public, live: count.is_some(), viewers: count })
    }

    pub async fn channel_info(self: Arc<Self>, quiet: bool) {
        let info = match self.twitch.read().await.clone() {
            None => Ok(None),
            Some(c) => c.channel_info().await.map(Some).map_err(twitch_err),
        };
        self.send(Ev::ChannelInfo { info, quiet });
    }

    pub async fn ads(self: Arc<Self>) {
        let r = match self.twitch.read().await.clone() {
            None => Ok(AdStatus { needs_reconnect: false, schedule: None }),
            Some(c) => match c.ad_schedule().await {
                Ok(s) => Ok(AdStatus { needs_reconnect: false, schedule: Some(s) }),
                Err(e) if e.is_unauthorized() => Ok(AdStatus { needs_reconnect: true, schedule: None }),
                Err(e) => Err(e.to_string()),
            },
        };
        self.send(Ev::Ads(r));
    }

    pub async fn search_categories(self: Arc<Self>, req: u64, query: String) {
        let result = match self.twitch.read().await.clone() {
            None => Err("Connect Twitch first (Settings, Connect Twitch).".to_string()),
            Some(c) => c.search_categories(query.trim()).await.map_err(twitch_err),
        };
        self.send(Ev::Categories { req, result });
    }

    pub async fn update_channel(self: Arc<Self>, title: Option<String>, category_id: Option<String>) {
        let r = async {
            if let Some(t) = &title {
                if t.trim().is_empty() {
                    return Err("The title cannot be empty.".to_string());
                }
                if t.chars().count() > 140 {
                    return Err("Twitch titles can be at most 140 characters.".to_string());
                }
            }
            let client = self.twitch.read().await.clone().ok_or("Connect Twitch first (Settings, Connect Twitch).")?;
            client.update_channel(title.as_deref().map(str::trim), category_id.as_deref()).await.map_err(|e| {
                if e.is_unauthorized() {
                    "Twitch did not allow the change. Disconnect and connect Twitch again in Settings so Sylva Control can ask for permission to edit your title and category.".to_string()
                } else {
                    e.to_string()
                }
            })?;
            client.channel_info().await.map_err(twitch_err)
        }
        .await;
        self.send(Ev::ChannelSaved(r));
    }

    /// Pause, resume, mute, unmute or skip Sylva's alert queue, or reload overlays.
    pub async fn alert_action(self: Arc<Self>, key: String, action: &'static str, ok_text: &'static str) {
        let r = match self.with_sylva().await {
            Ok((c, k)) => c.alerts(&k, action).await.map_err(sylva_err),
            Err(e) => Err(e),
        };
        if let Ok(s) = &r {
            if action != "reload" && action != "skip" {
                self.send(Ev::AlertState(s.clone()));
            }
        }
        self.send(Ev::Done { key, result: r.map(|_| ()), ok_text: Some(ok_text.into()) });
    }

    pub async fn alert_state(self: Arc<Self>) {
        if let Ok((c, k)) = self.with_sylva().await {
            if let Ok(s) = c.alert_state(&k).await {
                self.send(Ev::AlertState(s));
            }
        }
    }

    pub async fn held_tips(self: Arc<Self>) {
        if let Ok((c, k)) = self.with_sylva().await {
            if let Ok(t) = c.held_tips(&k).await {
                self.send(Ev::HeldTips(t));
            }
        }
    }

    pub async fn decide_tip(self: Arc<Self>, id: String, approve: bool) {
        let r = match self.with_sylva().await {
            Ok((c, k)) => c.tip_action(&k, &id, if approve { "approve" } else { "reject" }).await.map_err(sylva_err),
            Err(e) => Err(e),
        };
        let ok_text = if approve { "Tip sent to the stream" } else { "Tip refunded" };
        self.send(Ev::Done { key: format!("tip:{id}"), result: r, ok_text: Some(ok_text.into()) });
    }

    pub async fn replay(self: Arc<Self>, id: String) {
        let r = async {
            let seq: u64 = id.parse().map_err(|_| "This event cannot be replayed.".to_string())?;
            let (c, k) = self.with_sylva().await?;
            c.replay(&k, seq).await.map_err(sylva_err)
        }
        .await;
        self.send(Ev::Done { key: format!("replay:{id}"), result: r, ok_text: Some("Alert replayed".into()) });
    }

    pub async fn twitch_sign_in(self: Arc<Self>, client_id: String) {
        let client_id = client_id.trim().to_string();
        if client_id.is_empty() {
            return self.send(Ev::TwitchSignIn(Err("Enter your Twitch application's Client ID first.".into())));
        }
        let http = match twitch::http_client() {
            Ok(h) => h,
            Err(e) => return self.send(Ev::TwitchSignIn(Err(e.to_string()))),
        };
        let code = match twitch::start_device_flow(&http, twitch::ID_BASE, &client_id).await {
            Ok(c) => c,
            Err(Error::Http { status: 400, .. }) => {
                return self.send(Ev::TwitchSignIn(Err("Twitch did not accept that Client ID. Check that the app's Client Type is Public.".into())))
            }
            Err(e) => return self.send(Ev::TwitchSignIn(Err(e.to_string()))),
        };
        let cancel = CancellationToken::new();
        if let Some(old) = self.twitch_login.lock().await.replace(cancel.clone()) {
            old.cancel();
        }
        if let Err(e) = open::that_detached(&code.verification_uri) {
            log::warn!("could not open the browser: {e}");
        }
        self.send(Ev::TwitchCode(code.clone()));
        let result = match twitch::finish_device_flow(&http, twitch::ID_BASE, &client_id, &code, &cancel).await {
            Ok(tokens) => {
                let login = tokens.login.clone();
                match (secrets::save_twitch(&tokens), twitch_client(tokens)) {
                    (Ok(()), Ok(c)) => {
                        *self.twitch.write().await = Some(c);
                        Ok(login)
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e),
                }
            }
            Err(e) => Err(e.to_string()),
        };
        let ok = result.is_ok();
        self.send(Ev::TwitchSignIn(result));
        if ok {
            self.send_auth(None).await;
        }
    }

    pub async fn twitch_cancel(self: Arc<Self>) {
        if let Some(c) = self.twitch_login.lock().await.take() {
            c.cancel();
        }
    }

    pub async fn twitch_sign_out(self: Arc<Self>) {
        *self.twitch.write().await = None;
        let notice = secrets::clear_twitch().err();
        self.send_auth(notice).await;
    }

    async fn player(&self) -> Option<PlayerHandle> {
        self.sylva.read().await.as_ref().map(|s| s.player.clone())
    }

    /// Play, pause, seek (ms) or volume (0 to 100) on the song player.
    pub async fn sr_control(self: Arc<Self>, c: SongControl) {
        if let Some(p) = self.player().await {
            p.send(Input::Control(c));
        }
    }

    /// Change the queue on Sylva: `next`, `clear`, `remove` or `move` a song, or turn requests on or off.
    pub async fn sr_queue(self: Arc<Self>, action: &'static str, id: Option<u64>, to: Option<u64>, enabled: Option<bool>) {
        let r = async {
            let (client, key, player) = {
                let guard = self.sylva.read().await;
                let s = guard.as_ref().ok_or("Sign in to Sylva first.")?;
                (s.client.clone(), s.channel.key.clone(), s.player.clone())
            };
            let st = player.state();
            let (path, body) = match action {
                "next" => ("next".to_string(), serde_json::json!({ "from": st.view.current.as_ref().map(|s| s.id) })),
                "clear" => ("clear".to_string(), Value::Null),
                "remove" => (format!("{}/remove", id.ok_or("which song?")?), Value::Null),
                "move" => (format!("{}/move", id.ok_or("which song?")?), serde_json::json!({ "to": to.unwrap_or(0) })),
                "enabled" => {
                    let mut settings = serde_json::to_value(&st.view.settings).map_err(|e| e.to_string())?;
                    settings["enabled"] = Value::Bool(enabled.unwrap_or(false));
                    ("settings".to_string(), settings)
                }
                other => return Err(format!("unknown action: {other}")),
            };
            let v = client.songs_action(&key, &path, body).await.map_err(sylva_err)?;
            player.send(Input::Songs(Box::new(v)));
            Ok(())
        }
        .await;
        let ok_text = match (action, enabled) {
            ("enabled", Some(true)) => Some("Song requests on".to_string()),
            ("enabled", _) => Some("Song requests off".to_string()),
            _ => None,
        };
        self.send(Ev::Done { key: format!("sr:{action}"), result: r, ok_text });
    }
}

/// A Twitch client that writes refreshed tokens back to the credential store.
pub fn twitch_client(tokens: TwitchTokens) -> Result<Arc<TwitchClient>, String> {
    TwitchClient::new(
        tokens,
        Some(Box::new(|t: &TwitchTokens| {
            if let Err(e) = secrets::save_twitch(t) {
                log::warn!("{e}");
            }
        })),
    )
    .map_err(|e| e.to_string())
}
