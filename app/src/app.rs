//! The window: title bar, sidebar, the Home, Activity and Stream info pages, the sign-in page,
//! Settings, the Twitch and update dialogs, and toasts. Drawn by hand in Sylva's look
//! (crates/sylva-look) so it matches the Tauri version it replaces.
//!
//! Layout is done with plain rects: each block works out its height, paints, and hands back
//! how much room it used. Network work runs in `Backend`; its answers arrive as `Ev`.

use crate::activity::{self, chips, describe, group_of};
use crate::backend::{AdStatus, AuthState, Backend, Ev, UpdateInfo, ViewerSource};
use crate::format::{self, ago, num};
use crate::updater;
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Id, Key, Order, Pos2, Rect, Response, Sense, Stroke, UiBuilder};
use eframe::egui::text::LayoutJob;
use pine_core::sr::{PlayerState, SongControl};
use pine_core::twitch::{Category, ChannelInfo, DeviceCode};
use pine_core::Activity;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use sylva_look::theme::{self, *};
use sylva_look::widgets::{self as w, avatar, field, fmt, icon_button, line, para, skel, switch, tile, uv, Button, FieldOpts, Size, Toasts};
use sylva_look::{chrome, Icons, Images};

const MAX_ITEMS: usize = 500;
const RECENT: usize = 6;
/// Test events leave the feed 30 seconds after they came in, as on Sylva.
const TEST_TTL: f64 = 30.0;
const AD_SOON_S: f64 = 5.0 * 60.0;
const UPDATE_CHECK_S: f64 = 15.0 * 60.0;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Activity,
    Stream,
}

impl Page {
    fn key(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Activity => "activity",
            Page::Stream => "stream",
        }
    }
    fn from_key(k: &str) -> Self {
        match k {
            "activity" => Page::Activity,
            "stream" => Page::Stream,
            _ => Page::Home,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Accounts,
    Display,
    About,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Accounts => "Accounts",
            Pane::Display => "Display",
            Pane::About => "About",
        }
    }
}

struct Prefs {
    adbar: bool,
    actions: bool,
    compact: bool,
}

struct FeedItem {
    a: Activity,
    /// When a test event leaves the feed (egui time).
    expires: Option<f64>,
    /// When it arrived, for the slide in.
    born: f64,
}

#[derive(Default)]
struct ViewerView {
    loaded: bool,
    count: String,
    foot: String,
    detail: String,
    live: bool,
}

pub struct App {
    rt: tokio::runtime::Handle,
    backend: Arc<Backend>,
    rx: Receiver<Ev>,
    icons: Icons,
    images: Images,
    toasts: Toasts,
    hwnd: Option<isize>,

    page: Page,
    prefs: Prefs,
    show: String,
    query: String,
    twitch_client_id: String,

    auth: AuthState,
    restored: bool,
    signin_busy: bool,
    signin_code: Option<String>,
    signin_error: Option<String>,

    feed: Vec<FeedItem>,
    feed_loaded: bool,
    feed_loading: bool,

    followers: Option<String>,
    subs: Option<String>,
    viewers: ViewerView,
    viewer_source: ViewerSource,

    tips: Vec<Value>,
    muted: bool,
    paused: bool,
    busy: HashSet<String>,

    channel: Option<ChannelInfo>,
    title_edit: String,
    cat_edit: String,
    picked: Option<Category>,
    saving: bool,
    cat_results: Vec<Category>,
    cat_open: bool,
    cat_active: usize,
    cat_req: u64,
    cat_due: Option<f64>,
    cat_had_focus: bool,

    ads: Option<AdStatus>,
    ad_lengths: Vec<(i64, u64)>,

    sr: Option<PlayerState>,
    sr_open: bool,
    sr_seek: Option<u64>,

    settings: Option<Pane>,
    twitch_dialog: bool,
    tw_code: Option<DeviceCode>,
    tw_busy: bool,
    update: Option<UpdateInfo>,
    update_supported: bool,
    announced_behind: u32,
    update_dialog: Option<(&'static str, String)>,

    next_viewers: f64,
    next_stats: f64,
    next_tips: f64,
    next_info: f64,
    next_ads: f64,
    next_update: f64,
    stats_soon: Option<f64>,
    content_h: f32,
    scroll_top: bool,
}

const K_PAGE: &str = "page";
const K_PREFS: &str = "prefs";
const K_SHOW: &str = "activityShow";
const K_CLIENT: &str = "twitchClientId";

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, rt: tokio::runtime::Handle) -> Self {
        theme::install(&cc.egui_ctx);
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let notify: crate::backend::Notify = Arc::new(move |e| {
            let _ = tx.send(e);
            ctx.request_repaint();
        });
        let backend = Backend::new(notify);
        let get = |k: &str| cc.storage.and_then(|s| s.get_string(k));
        let prefs_raw: Value = get(K_PREFS).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        let pref = |k: &str, d: bool| prefs_raw.get(k).and_then(Value::as_bool).unwrap_or(d);
        let show = get(K_SHOW).filter(|s| chips().iter().any(|c| c.0 == s)).unwrap_or_else(|| "all".into());
        let demo = std::env::var_os("SYLVA_CONTROL_DEMO").is_some();
        if !demo {
            let b = backend.clone();
            rt.spawn(b.restore());
        }
        let mut app = Self {
            images: Images::new(cc.egui_ctx.clone(), rt.clone()),
            icons: Icons::load(&cc.egui_ctx),
            toasts: Toasts::default(),
            hwnd: crate::drag::hwnd(cc),
            rt,
            backend,
            rx,
            page: Page::from_key(&get(K_PAGE).unwrap_or_default()),
            prefs: Prefs { adbar: pref("adbar", true), actions: pref("actions", true), compact: pref("compact", false) },
            show,
            query: String::new(),
            twitch_client_id: get(K_CLIENT).unwrap_or_default(),
            auth: AuthState::default(),
            restored: false,
            signin_busy: false,
            signin_code: None,
            signin_error: None,
            feed: Vec::new(),
            feed_loaded: false,
            feed_loading: false,
            followers: None,
            subs: None,
            viewers: ViewerView::default(),
            viewer_source: ViewerSource::Public,
            tips: Vec::new(),
            muted: false,
            paused: false,
            busy: HashSet::new(),
            channel: None,
            title_edit: String::new(),
            cat_edit: String::new(),
            picked: None,
            saving: false,
            cat_results: Vec::new(),
            cat_open: false,
            cat_active: 0,
            cat_req: 0,
            cat_due: None,
            cat_had_focus: false,
            ads: None,
            ad_lengths: Vec::new(),
            sr: None,
            sr_open: false,
            sr_seek: None,
            settings: None,
            twitch_dialog: false,
            tw_code: None,
            tw_busy: false,
            update: None,
            update_supported: false,
            announced_behind: 0,
            update_dialog: None,
            next_viewers: 0.0,
            next_stats: f64::MAX,
            next_tips: f64::MAX,
            next_info: f64::MAX,
            next_ads: f64::MAX,
            next_update: 3.0,
            stats_soon: None,
            content_h: 0.0,
            scroll_top: false,
        };
        if demo {
            app.demo();
        }
        app
    }

    /// Sample data for looking at the layout without signing in (SYLVA_CONTROL_DEMO=1).
    fn demo(&mut self) {
        self.auth.sylva = Some(pine_core::sylva::SylvaChannel { key: "demo".into(), name: Some("Josh".into()), login: Some("haruxee".into()), avatar: None });
        self.auth.twitch = Some("haruxee".into());
        self.restored = true;
        self.feed_loaded = true;
        self.followers = Some("12,408".into());
        self.subs = Some("316".into());
        self.viewers = ViewerView { loaded: true, count: "1,284".into(), foot: "Live now".into(), detail: String::new(), live: true };
        let now = chrono::Utc::now();
        use pine_core::ActivityKind as K;
        let ev = |i: usize, kind: K, raw: &str, name: &str, amount: Option<f64>, message: Option<&str>| Activity {
            id: format!("d{i}"),
            kind,
            raw_type: raw.into(),
            provider: Some("twitch".into()),
            created_at: Some((now - chrono::Duration::minutes(i as i64 * 7)).to_rfc3339()),
            username: Some(name.to_lowercase()),
            display_name: Some(name.into()),
            avatar: None,
            amount,
            currency: (kind == K::Tip).then(|| "USD".into()),
            message: message.map(str::to_string),
            tier: (kind == K::Subscriber).then(|| "1000".into()),
            gifted: false,
            sender: None,
            redemption: None,
            raw: None,
            test: i == 0,
            received_at: None,
        };
        let items = [
            ev(0, K::Follow, "channel.follow", "MossyRiver", None, None),
            ev(1, K::Cheer, "channel.cheer", "pinecone_42", Some(500.0), Some("that clutch was unreal, keep it up")),
            ev(2, K::Subscriber, "channel.subscribe", "fernlight", None, None),
            ev(3, K::Raid, "channel.raid", "SlowCanopy", Some(84.0), None),
            ev(4, K::Tip, "tip", "Willow", Some(5.0), Some("for the coffee fund")),
            ev(5, K::Follow, "channel.follow", "birchbark", None, None),
        ];
        self.feed = items.into_iter().map(|a| FeedItem { a, expires: None, born: -1.0 }).collect();
        self.ads = Some(AdStatus { schedule: Some(pine_core::twitch::AdSchedule { next_ad_at: Some((wall_s() + 200.0) as i64), last_ad_at: None, duration: 90, snooze_count: 2, ..Default::default() }), needs_reconnect: false });
        let f = f64::MAX;
        (self.next_viewers, self.next_stats, self.next_tips, self.next_info, self.next_ads, self.next_update) = (f, f, f, f, f, f);
    }

    fn run<F: std::future::Future<Output = ()> + Send + 'static>(&self, f: F) {
        self.rt.spawn(f);
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>, bad: bool) {
        self.toasts.push(ctx, text, bad, if bad { 6.5 } else { 3.0 });
    }

    fn go(&mut self, p: Page) {
        if p != self.page {
            self.scroll_top = true;
        }
        self.page = p;
    }

    // ---------- Events ----------

    fn drain(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Ev::Auth { auth, notice } => self.apply_auth(ctx, auth, notice),
                Ev::Feed(r) => {
                    self.feed_loading = false;
                    self.feed_loaded = true;
                    match r {
                        Ok(history) => {
                            let known: HashSet<String> = history.iter().map(|a| a.id.clone()).collect();
                            let mut items: Vec<FeedItem> = std::mem::take(&mut self.feed).into_iter().filter(|f| !known.contains(&f.a.id)).collect();
                            let wall = chrono::Utc::now().timestamp_millis();
                            items.extend(history.into_iter().map(|a| {
                                let expires = a.test.then(|| {
                                    let left = a.received_at.map_or(TEST_TTL, |r| ((r as f64 + TEST_TTL * 1000.0 - wall as f64) / 1000.0).clamp(0.0, TEST_TTL));
                                    now + left
                                });
                                FeedItem { a, expires, born: 0.0 }
                            }));
                            items.truncate(MAX_ITEMS);
                            self.feed = items;
                        }
                        Err(e) => self.toast(ctx, format!("Could not load activity: {e}"), true),
                    }
                }
                Ev::Live(a) => {
                    if self.feed.iter().any(|f| f.a.id == a.id) {
                        continue;
                    }
                    let expires = a.test.then_some(now + TEST_TTL);
                    self.feed.insert(0, FeedItem { a: *a, expires, born: now });
                    self.feed.truncate(MAX_ITEMS);
                    self.stats_soon = Some(now + 3.0);
                }
                Ev::TipsChanged => self.load_tips(),
                Ev::HeldTips(t) => self.tips = t,
                Ev::Stats { followers, subscribers } => {
                    self.followers = Some(num(followers));
                    self.subs = Some(num(subscribers));
                }
                Ev::Viewers(r) => {
                    let v = &mut self.viewers;
                    v.loaded = true;
                    v.detail.clear();
                    match r {
                        Ok(x) => {
                            self.viewer_source = x.source;
                            if x.source == ViewerSource::None {
                                v.count = "-".into();
                                v.foot = if self.auth.sylva.is_some() { "No Twitch channel on this account".into() } else { "Not available".into() };
                                v.live = false;
                            } else {
                                // Offline shows a dash, not 0, so it never looks like an empty stream.
                                v.count = if x.live { num(x.viewers) } else { "-".into() };
                                v.foot = if x.live { "Live now".into() } else { "Offline".into() };
                                v.live = x.live;
                            }
                        }
                        Err(e) => {
                            v.count = "-".into();
                            v.foot = "Viewer count unavailable".into();
                            v.detail = e;
                            v.live = false;
                        }
                    }
                    let wait = match self.viewer_source {
                        ViewerSource::Twitch => 5.0,
                        ViewerSource::Public => 10.0,
                        ViewerSource::None => 30.0,
                    };
                    let hidden = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
                    self.next_viewers = now + if hidden { 60.0 } else { wait };
                }
                Ev::ChannelInfo { info, quiet } => match info {
                    Ok(Some(info)) => {
                        let searching = self.cat_had_focus && self.picked.is_none();
                        if searching || self.info_changed() {
                            self.channel = Some(info);
                        } else {
                            self.show_channel_info(info);
                        }
                    }
                    Ok(None) => self.channel = None,
                    Err(e) => {
                        if !quiet {
                            self.toast(ctx, format!("Could not read your stream info: {e}"), true);
                        }
                    }
                },
                Ev::ChannelSaved(r) => {
                    self.saving = false;
                    match r {
                        Ok(info) => {
                            self.show_channel_info(info);
                            self.toast(ctx, "Stream info updated", false);
                        }
                        Err(e) => self.toast(ctx, e, true),
                    }
                }
                Ev::Ads(r) => {
                    let mut next = 30.0;
                    if let Ok(s) = r {
                        if let Some(sch) = &s.schedule {
                            if let Some(at) = sch.next_ad_at.filter(|_| sch.duration > 0) {
                                if !self.ad_lengths.iter().any(|x| x.0 == at) {
                                    self.ad_lengths.push((at, sch.duration));
                                }
                                if self.ad_lengths.len() > 20 {
                                    self.ad_lengths.remove(0);
                                }
                            }
                        }
                        // A sign-in without the ads permission stays that way until Twitch is reconnected.
                        if s.needs_reconnect {
                            next = f64::MAX;
                        } else if let Some(sch) = &s.schedule {
                            let t = wall_s();
                            if self.running_ad(sch, t).is_some() || sch.next_ad_at.is_some_and(|n| (n as f64) - t <= 90.0) {
                                next = 5.0;
                            }
                        }
                        self.ads = Some(s);
                    }
                    self.next_ads = if next == f64::MAX { f64::MAX } else { now + next };
                }
                Ev::Categories { req, result } => {
                    if req != self.cat_req {
                        continue;
                    }
                    match result {
                        Ok(r) => {
                            self.cat_results = r;
                            self.cat_active = 0;
                            self.cat_open = true;
                        }
                        Err(e) => self.toast(ctx, e, true),
                    }
                }
                Ev::AlertState(s) => {
                    self.muted = s.muted;
                    self.paused = s.paused;
                }
                Ev::SylvaCode(code) => self.signin_code = Some(format!("Approve Sylva Control in your browser. Your code is {code}.")),
                Ev::SylvaSignIn(r) => {
                    self.signin_busy = false;
                    self.signin_code = None;
                    match r {
                        Ok(name) => {
                            let t = match name {
                                Some(n) => format!("Signed in as {n}"),
                                None => "Signed in".into(),
                            };
                            self.toast(ctx, t, false);
                        }
                        Err(e) => self.signin_error = Some(format!("Sign-in failed: {e}")),
                    }
                }
                Ev::TwitchCode(c) => self.tw_code = Some(c),
                Ev::TwitchSignIn(r) => {
                    self.tw_code = None;
                    self.tw_busy = false;
                    match r {
                        Ok(login) => {
                            self.twitch_dialog = false;
                            self.toast(ctx, format!("Twitch connected as {login}"), false);
                            self.next_viewers = now;
                            self.next_stats = now;
                        }
                        Err(e) => self.toast(ctx, format!("Twitch sign-in failed: {e}"), true),
                    }
                }
                Ev::Sr(s) => {
                    if let (Some(seek), Some(s)) = (self.sr_seek, &s) {
                        // Stop holding the dragged position once the player has caught up.
                        if s.position_ms.abs_diff(seek) < 1500 && !ctx.input(|i| i.pointer.primary_down()) {
                            self.sr_seek = None;
                        }
                    }
                    self.sr = s;
                }
                Ev::SylvaUnauthorized => self.toast(ctx, "Sylva no longer accepts Sylva Control. Sign out and sign in again.", true),
                Ev::Done { key, result, ok_text } => {
                    self.busy.remove(&key);
                    match result {
                        Ok(()) => {
                            if let Some(id) = key.strip_prefix("tip:") {
                                self.tips.retain(|t| t["id"].as_str() != Some(id));
                            }
                            if let Some(t) = ok_text {
                                self.toast(ctx, t, false);
                            }
                        }
                        Err(e) => self.toast(ctx, e, true),
                    }
                }
                Ev::Update(r, manual) => match r {
                    Ok(u) => {
                        self.update_supported = u.supported;
                        if u.supported && u.behind > 0 {
                            if manual || u.behind != self.announced_behind {
                                self.toasts.push(ctx, "A Sylva Control update is ready. Click Update at the top to install it.", false, 10.0);
                            }
                            self.announced_behind = u.behind;
                            self.update = Some(u);
                        } else {
                            self.update = None;
                            if manual && u.supported {
                                self.toast(ctx, "Sylva Control is up to date", false);
                            }
                        }
                    }
                    Err(e) => {
                        if manual {
                            self.toast(ctx, e, true);
                        } else {
                            log::warn!("update check: {e}");
                        }
                    }
                },
                Ev::UpdateStatus { step, text } => {
                    self.settings = None;
                    self.update_dialog = Some((step, text));
                }
            }
        }
        // Test events leave on their own.
        let before = self.feed.len();
        self.feed.retain(|f| f.expires.is_none_or(|e| e > now));
        if let Some(soonest) = self.feed.iter().filter_map(|f| f.expires).reduce(f64::min) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((soonest - now).max(0.05)));
        }
        if before != self.feed.len() {
            ctx.request_repaint();
        }
    }

    fn apply_auth(&mut self, ctx: &egui::Context, a: AuthState, notice: Option<String>) {
        let now = ctx.input(|i| i.time);
        let before = self.auth.sylva.as_ref().map(|s| s.key.clone());
        let after = a.sylva.as_ref().map(|s| s.key.clone());
        let twitch_changed = self.auth.twitch != a.twitch;
        self.auth = a;
        self.restored = true;
        if let Some(n) = notice {
            self.toast(ctx, n, true);
        }
        if before != after {
            self.feed.clear();
            self.feed_loaded = false;
            self.followers = None;
            self.subs = None;
            self.viewers = ViewerView::default();
            self.tips.clear();
            if after.is_some() {
                self.load_feed();
            }
        }
        if before != after || twitch_changed {
            self.next_viewers = now;
            self.next_stats = now;
            self.next_info = now;
            self.next_ads = now;
            self.next_tips = now;
            if self.auth.twitch.is_none() {
                self.channel = None;
                self.ads = None;
            }
            let b = self.backend.clone();
            self.run(b.alert_state());
        }
    }

    fn load_feed(&mut self) {
        if self.auth.sylva.is_none() || self.feed_loading {
            return;
        }
        self.feed_loading = true;
        let b = self.backend.clone();
        self.run(b.feed());
    }

    fn load_tips(&self) {
        if self.auth.sylva.is_some() {
            let b = self.backend.clone();
            self.run(b.held_tips());
        }
    }

    /// Polls that run on their own: viewers, totals, tips, stream info, ads and updates.
    fn timers(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let hidden = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let b = || self.backend.clone();
        if now >= self.next_viewers && self.restored {
            self.next_viewers = f64::MAX;
            self.run(b().viewers());
        }
        if now >= self.next_stats || self.stats_soon.is_some_and(|t| now >= t) {
            self.stats_soon = None;
            self.next_stats = now + 60.0;
            if self.auth.sylva.is_some() {
                self.run(b().stats());
            }
        }
        if now >= self.next_tips {
            self.next_tips = now + 60.0;
            if !hidden {
                self.load_tips();
            }
        }
        if now >= self.next_info {
            self.next_info = now + 30.0;
            if self.auth.twitch.is_some() && !hidden {
                let quiet = self.channel.is_some();
                self.run(b().channel_info(quiet));
            }
        }
        if now >= self.next_ads {
            self.next_ads = f64::MAX;
            if self.auth.twitch.is_some() {
                self.run(b().ads());
            } else {
                self.ads = None;
            }
        }
        if now >= self.next_update {
            self.next_update = now + UPDATE_CHECK_S;
            if self.update_dialog.is_none() {
                self.run(updater::check(self.backend.notify.clone(), false));
            }
        }
        if let Some(due) = self.cat_due {
            if now >= due {
                self.cat_due = None;
                self.cat_req += 1;
                let (req, q) = (self.cat_req, self.cat_edit.trim().to_string());
                self.run(b().search_categories(req, q));
            }
        }
        let soonest = [self.next_viewers, self.next_stats, self.next_tips, self.next_info, self.next_ads, self.next_update, self.cat_due.unwrap_or(f64::MAX), self.stats_soon.unwrap_or(f64::MAX)]
            .into_iter()
            .fold(f64::MAX, f64::min);
        if soonest < f64::MAX {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((soonest - now).clamp(0.01, 60.0)));
        }
    }

    // ---------- Stream info helpers ----------

    fn show_channel_info(&mut self, info: ChannelInfo) {
        self.title_edit = info.title.clone();
        self.cat_edit = info.game_name.clone();
        self.picked = (!info.game_id.is_empty()).then(|| Category { id: info.game_id.clone(), name: info.game_name.clone(), box_art_url: String::new() });
        self.channel = Some(info);
    }

    fn info_changed(&self) -> bool {
        let Some(c) = &self.channel else { return false };
        let t = self.title_edit.trim();
        let title_changed = !t.is_empty() && t != c.title;
        let cat_changed = self.picked.as_ref().is_some_and(|p| p.id != c.game_id);
        title_changed || cat_changed
    }

    fn save_info(&mut self) {
        if !self.info_changed() || self.saving {
            return;
        }
        let c = self.channel.clone().unwrap();
        let t = self.title_edit.trim().to_string();
        let title = (t != c.title).then_some(t);
        let cat = self.picked.as_ref().filter(|p| p.id != c.game_id).map(|p| p.id.clone());
        self.saving = true;
        let b = self.backend.clone();
        self.run(b.update_channel(title, cat));
    }

    fn running_ad(&self, sch: &pine_core::twitch::AdSchedule, now: f64) -> Option<(f64, f64)> {
        let last = sch.last_ad_at? as f64;
        let mut length = 0u64;
        for (at, len) in &self.ad_lengths {
            if ((*at as f64) - last).abs() <= 90.0 {
                length = *len;
            }
        }
        let length = if length > 0 { length } else if sch.duration > 0 { sch.duration } else { 60 } as f64;
        (now < last + length).then_some((last, length))
    }

    fn open_twitch_dialog(&mut self) {
        self.settings = None;
        if self.twitch_client_id.is_empty() {
            self.twitch_client_id = self.auth.default_twitch_client_id.clone().unwrap_or_default();
        }
        self.twitch_dialog = true;
    }

    fn close_twitch_dialog(&mut self) {
        self.twitch_dialog = false;
        if self.tw_code.take().is_some() {
            let b = self.backend.clone();
            self.run(b.twitch_cancel());
        }
        self.tw_busy = false;
    }

    // ---------- Title bar and sidebar ----------

    fn titlebar(&mut self, ui: &mut egui::Ui, rect: Rect, narrow: bool) {
        let hwnd = self.hwnd;
        let right = chrome::titlebar(ui, &self.icons, rect, "Sylva Control", !narrow, || crate::drag::keep_current_size(hwnd));
        if let Some(u) = &self.update {
            let tip = format!("What's new:\n{}", u.changes.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n"));
            let btn = Button::new("Update").primary().sm().icon("download").enabled(self.update_dialog.is_none());
            let size = btn.measure(ui);
            let r = Rect::from_min_size(pos2(right - size.x, rect.center().y - size.y / 2.0), size);
            if btn.show_at(ui, &self.icons, r, Id::unique("update-btn")).on_hover_text(tip).clicked() {
                self.update_dialog = Some(("fetch", "Starting".into()));
                updater::apply(self.backend.notify.clone());
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, rect: Rect, icons_only: bool) {
        let pad = if rect.width() <= 56.0 { 6.0 } else if icons_only { 10.0 } else { 12.0 };
        let x0 = rect.left() + pad;
        let w = rect.width() - 2.0 * pad;
        let mut y = rect.top() + 4.0;
        let items = [(Page::Home, "home", "Home"), (Page::Activity, "activity", "Activity"), (Page::Stream, "edit", "Stream info")];
        for (page, icon, label) in items {
            let r = Rect::from_min_size(pos2(x0, y), vec2(w, 36.0));
            if self.side_item(ui, r, icon, label, self.page == page, icons_only).clicked() {
                self.go(page);
            }
            y += 37.0;
        }
        // Foot: who is signed in, then Settings.
        let set = Rect::from_min_size(pos2(x0, rect.bottom() - 14.0 - 36.0), vec2(w, 36.0));
        if self.side_item(ui, set, "settings", "Settings", self.settings.is_some(), icons_only).clicked() {
            self.settings = Some(Pane::Accounts);
        }
        if let Some(sy) = &self.auth.sylva {
            let me = Rect::from_min_size(pos2(x0, set.top() - 6.0 - 44.0), vec2(w, 44.0));
            let name = sy.name.clone().or(sy.login.clone()).unwrap_or_else(|| "Streamer".into());
            let p = ui.painter();
            let ax = if icons_only { me.center().x } else { me.left() + 6.0 + 16.0 };
            avatar(p, &self.images, pos2(ax, me.center().y), 32.0, &name, sy.avatar.as_deref());
            if !icons_only {
                let tw = me.width() - 6.0 - 32.0 - 10.0 - 6.0;
                let g1 = line(p, name, medium(13.5), TEXT, tw);
                let g2 = line(p, if self.auth.twitch.is_some() { "Twitch connected" } else { "Twitch not connected" }, regular(12.0), FAINT, tw);
                let tx = me.left() + 6.0 + 32.0 + 10.0;
                p.galley(pos2(tx, me.center().y - 18.0), g1, TEXT);
                p.galley(pos2(tx, me.center().y + 1.0), g2, TEXT);
            }
        }
    }

    fn side_item(&self, ui: &mut egui::Ui, r: Rect, icon: &str, label: &str, on: bool, icons_only: bool) -> Response {
        let resp = ui.interact(r, Id::unique(("side", label)), Sense::click());
        let resp = if icons_only { resp.on_hover_text(label) } else { resp };
        let p = ui.painter();
        if on {
            p.rect_filled(r, R_CONTROL, ROW_ON);
        } else if resp.hovered() {
            p.rect_filled(r, R_CONTROL, SURFACE);
        }
        let ink = if on || resp.hovered() { TEXT } else { TEXT_2 };
        let icon_ink = if on { TEXT } else { MUTED };
        if icons_only {
            self.icons.paint(p, icon, r.center(), 17.0, icon_ink);
        } else {
            self.icons.paint(p, icon, pos2(r.left() + 10.0 + 8.5, r.center().y), 17.0, icon_ink);
            let g = line(p, label, medium(14.0), ink, r.width() - 47.0);
            p.galley(pos2(r.left() + 37.0, r.center().y - g.size().y / 2.0), g, ink);
        }
        w::hand(ui, &resp);
        resp
    }

    // ---------- Pages ----------

    fn main_panel(&mut self, ui: &mut egui::Ui, panel: Rect, full_w: f32) {
        let pad_x = if full_w <= 480.0 { 14.0 } else if full_w <= 860.0 { 22.0 } else { 36.0 };
        let pad_top = if full_w <= 480.0 { 18.0 } else if full_w <= 860.0 { 22.0 } else { 28.0 };
        let mut area = egui::ScrollArea::vertical().id_salt("main").auto_shrink([false, false]);
        if std::mem::take(&mut self.scroll_top) {
            area = area.vertical_scroll_offset(0.0);
        }
        let content_h = self.content_h;
        ui.scope_builder(UiBuilder::new().max_rect(panel), |ui| {
            area.show_viewport(ui, |ui, viewport| {
                ui.set_min_size(vec2(panel.width(), content_h));
                let origin = ui.min_rect().min;
                let page_w = (panel.width().min(980.0) - 2.0 * pad_x).max(120.0);
                let x = origin.x + pad_x;
                let mut y = origin.y + pad_top;
                let vis = viewport.translate(origin.to_vec2());
                y += match self.page {
                    Page::Home => self.home(ui, x, y, page_w, full_w, vis),
                    Page::Activity => self.activity_page(ui, x, y, page_w, vis),
                    Page::Stream => self.stream_page(ui, x, y, page_w),
                };
                let bottom = if full_w <= 480.0 { 32.0 } else if full_w <= 860.0 { 40.0 } else { 48.0 };
                self.content_h = y + bottom - origin.y;
            });
        });
    }

    fn page_head(&mut self, ui: &mut egui::Ui, x: f32, y: f32, w: f32, tag: &str, title: &str, desc: &str, small: bool, action: Option<Button>) -> (f32, bool) {
        let p = ui.painter().clone();
        let g = p.layout_no_wrap(tag.to_string(), mono(13.0), SAGE);
        let tag_r = Rect::from_min_size(pos2(x, y), vec2(g.size().x + 20.0, 26.0));
        p.rect_filled(tag_r, R_CONTROL, TAG_BG);
        p.galley(pos2(tag_r.left() + 10.0, tag_r.center().y - g.size().y / 2.0), g, SAGE);
        let action_w = action.as_ref().map_or(0.0, |b| b.measure(ui).x + 16.0);
        let text_w = (w - action_w).clamp(100.0, 640.0);
        let h1 = para(&p, title, medium(if small { 22.0 } else { 26.0 }), TEXT, w - action_w, 0);
        let mut yy = tag_r.bottom() + 12.0;
        let h1_h = h1.size().y.max(if small { 28.0 } else { 32.0 });
        p.galley(pos2(x, yy + (h1_h - h1.size().y) / 2.0), h1, TEXT);
        yy += h1_h;
        if !desc.is_empty() {
            let d = para(&p, desc, regular(14.0), MUTED, text_w, 0);
            yy += 4.0;
            p.galley(pos2(x, yy), d.clone(), TEXT);
            yy += d.size().y;
        }
        let mut clicked = false;
        if let Some(b) = action {
            let s = b.measure(ui);
            let r = Rect::from_min_size(pos2(x + w - s.x, yy - s.y), s);
            clicked = b.show_at(ui, &self.icons, r, Id::unique(("head-action", tag))).clicked();
        }
        (yy - y + 22.0, clicked)
    }

    /// A section heading (small, faint) with an optional control on the right. Returns its height.
    fn sect_head(&self, ui: &egui::Ui, x: f32, y: f32, w: f32, title: &str, sub: &str, right_w: f32) -> f32 {
        let p = ui.painter();
        let g = line(p, title, regular(12.5), FAINT, w - right_w - 16.0);
        let mut h = g.size().y;
        let mut gs = vec![g];
        if !sub.is_empty() {
            let s = para(p, sub, regular(12.5), FAINT, w - right_w - 16.0, 0);
            h += 1.0 + s.size().y;
            gs.push(s);
        }
        let total = h.max(32.0);
        let mut yy = y + (total - h) / 2.0;
        for (i, g) in gs.into_iter().enumerate() {
            if i == 1 {
                yy += 1.0;
            }
            let gh = g.size().y;
            p.galley(pos2(x + 4.0, yy), g, TEXT);
            yy += gh;
        }
        total + 8.0
    }

    fn home(&mut self, ui: &mut egui::Ui, x: f32, y0: f32, w: f32, full_w: f32, vis: Rect) -> f32 {
        let mut y = y0;
        let name = self.auth.sylva.as_ref().map(|s| s.name.clone().or(s.login.clone()).unwrap_or_else(|| "Streamer".into()));
        let title = match &name {
            Some(n) => format!("{}, {n}", format::greeting()),
            None => "Welcome back".into(),
        };
        y += self.page_head(ui, x, y, w, "/home", &title, "Here's your channel right now.", full_w <= 480.0, None).0;

        if self.prefs.adbar {
            y += self.adbar(ui, x, y, w);
        }
        if !self.tips.is_empty() {
            y += self.held_tips(ui, x, y, w);
        }
        y += self.totals(ui, x, y, w, full_w);
        if self.prefs.actions {
            y += 28.0;
            y += self.sect_head(ui, x, y, w, "Quick actions", "", 0.0);
            y += self.quick_actions(ui, x, y, w, full_w);
        }
        if self.sr.is_some() {
            y += 28.0;
            y += self.song_requests(ui, x, y, w);
        }
        y += 28.0;
        let btn = Button::new("See all").ghost().sm().icon_after("chevron");
        let bs = btn.measure(ui);
        let head_h = self.sect_head(ui, x, y, w, "Recent activity", "", bs.x);
        if btn.show_at(ui, &self.icons, Rect::from_min_size(pos2(x + w - bs.x, y + (32.0 - bs.y) / 2.0), bs), Id::unique("see-all")).clicked() {
            self.go(Page::Activity);
        }
        y += head_h;
        y += self.feed_card(ui, x, y, w, FeedScope::Recent, vis);
        y - y0
    }

    fn adbar(&mut self, ui: &mut egui::Ui, x: f32, y: f32, w: f32) -> f32 {
        let Some(ads) = &self.ads else { return 0.0 };
        if self.auth.twitch.is_none() {
            return 0.0;
        }
        let now = wall_s();
        // state: 0 calm, 1 soon, 2 running
        let (state, title, sub, time, progress) = if ads.needs_reconnect {
            (0, "See ad breaks here".to_string(), "Reconnect Twitch once so Sylva Control can read your ad schedule.".to_string(), String::new(), 0.0)
        } else {
            let Some(sch) = &ads.schedule else { return 0.0 };
            if let Some((start, length)) = self.running_ad(sch, now) {
                let left = start + length - now;
                (2, "Ads are running".into(), format!("{} second break. Back at {}.", length as u64, format::time_at((start + length) as i64)), format::clock(left), ((now - start) / length) as f32)
            } else {
                let Some(next) = sch.next_ad_at else { return 0.0 };
                let until = next as f64 - now;
                let len = if sch.duration > 0 { format!("{} second break", sch.duration) } else { "Ad break".into() };
                if until <= 0.0 {
                    (2, "Ad break starting".into(), format!("{len}, starting now."), "0:00".into(), 1.0)
                } else if until <= AD_SOON_S {
                    (1, "Ad break soon".into(), format!("{len} at {}.", format::time_at(next)), format::clock(until), (1.0 - until / AD_SOON_S) as f32)
                } else {
                    let snoozes = if sch.snooze_count > 0 { format!(", {} {} left", sch.snooze_count, if sch.snooze_count == 1 { "snooze" } else { "snoozes" }) } else { String::new() };
                    (0, "Next ad break".into(), format!("{len} at {}{snoozes}.", format::time_at(next)), format::minutes(until), 0.0)
                }
            }
        };
        let c = match state {
            1 => WARN,
            2 => DANGER,
            _ => FAINT,
        };
        let bg = if state == 0 { SURFACE } else { w::lerp(SURFACE, c, 0.10) };
        let needs_reconnect = ads.needs_reconnect;
        let p = ui.painter().clone();
        let btn = needs_reconnect.then(|| Button::new("Reconnect Twitch").sm().in_card(true));
        let btn_w = btn.as_ref().map_or(0.0, |b| b.measure(ui).x + 14.0);
        let tg = p.layout_no_wrap(time.clone(), mono(20.0), if state == 0 { TEXT } else { c });
        let text_w = w - 32.0 - 32.0 - 14.0 - if time.is_empty() { 0.0 } else { tg.size().x + 14.0 } - btn_w;
        let g1 = line(&p, title, medium(14.0), if state == 0 { TEXT } else { c }, text_w);
        let g2 = if full_narrow(w) { para(&p, sub, regular(12.5), FAINT, text_w, 0) } else { line(&p, sub, regular(12.5), FAINT, text_w) };
        let text_h = 20.0 + 2.0 + g2.size().y;
        let h = (text_h + 24.0).max(64.0);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        p.rect_filled(r, R_ITEM, bg);
        let tr = Rect::from_min_size(pos2(x + 16.0, r.center().y - 16.0), vec2(32.0, 32.0));
        let breathe = if state == 2 { breath(ui, 2.0) } else { 1.0 };
        tile(&p, &self.icons, tr, "timer", TILE, (if state == 0 { TILE_INK } else { c }).gamma_multiply(breathe));
        let tx = tr.right() + 14.0;
        let ty = r.center().y - text_h / 2.0;
        p.galley(pos2(tx, ty), g1, TEXT);
        p.galley(pos2(tx, ty + 22.0), g2, TEXT);
        let mut rx = r.right() - 16.0;
        if let Some(b) = btn {
            let s = b.measure(ui);
            let br = Rect::from_min_size(pos2(rx - s.x, r.center().y - s.y / 2.0), s);
            if b.show_at(ui, &self.icons, br, Id::unique("ad-reconnect")).clicked() {
                self.open_twitch_dialog();
            }
            rx -= s.x + 14.0;
        }
        if !time.is_empty() && !needs_reconnect {
            p.galley(pos2(rx - tg.size().x, r.center().y - tg.size().y / 2.0), tg, TEXT);
        }
        if state > 0 && !needs_reconnect {
            let bar = Rect::from_min_size(pos2(r.left(), r.bottom() - 3.0), vec2(w * progress.clamp(0.0, 1.0), 3.0));
            p.with_clip_rect(r).rect_filled(bar, CornerRadius { sw: 12, ..Default::default() }, c);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
        h + 16.0
    }

    fn held_tips(&mut self, ui: &mut egui::Ui, x: f32, y0: f32, w: f32) -> f32 {
        let mut y = y0;
        y += self.sect_head(ui, x, y, w, "Tips waiting for you", "Paid, not shown yet. Show each one on stream or refund it in full.", 0.0);
        let p = ui.painter().clone();
        let tips = self.tips.clone();
        let narrow = w < 452.0;
        // Lay out first, then paint the card behind the rows.
        let mut rows = Vec::new();
        let mut h_total = 0.0;
        for t in &tips {
            let ctl_w = if narrow { 0.0 } else { 160.0 };
            let tw = w - 32.0 - 32.0 - 14.0 - ctl_w - 14.0;
            let name = t["name"].as_str().unwrap_or("Someone").to_string();
            let amount = format::money(t["amount"].as_f64().unwrap_or(0.0), t["currency"].as_str());
            let mut job = LayoutJob::default();
            job.append(&format!("{name} "), 0.0, fmt(medium(14.0), TEXT));
            job.append(&amount, 0.0, fmt(medium(14.0), SAGE));
            job.wrap.max_width = tw;
            let g1 = p.layout_job(job);
            let msg = t["message"].as_str().filter(|m| !m.is_empty()).map(|m| para(&p, m, regular(13.5), TEXT_2, tw, 0));
            let why: Vec<String> = [t["holdReason"].as_str().map(str::to_string), t["country"].as_str().map(|c| format!("from {c}"))].into_iter().flatten().collect();
            let why = why.join(", ");
            let why = (!why.is_empty()).then(|| {
                let mut c = why.chars();
                let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
                para(&p, cap, regular(12.5), FAINT, tw, 0)
            });
            let text_h = g1.size().y.max(20.0) + msg.as_ref().map_or(0.0, |m| 2.0 + m.size().y) + why.as_ref().map_or(0.0, |g| 2.0 + g.size().y);
            let h = (text_h + 24.0).max(60.0) + if narrow { 38.0 } else { 0.0 };
            rows.push((t["id"].as_str().unwrap_or_default().to_string(), g1, msg, why, text_h, h));
            h_total += h;
        }
        p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, h_total)), R_ITEM, SURFACE);
        for (id, g1, msg, why, text_h, h) in rows {
            let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
            let body_h = if narrow { h - 38.0 } else { h };
            let cy = r.top() + body_h / 2.0;
            tile(&p, &self.icons, Rect::from_min_size(pos2(x + 16.0, cy - 16.0), vec2(32.0, 32.0)), "tip", TILE, TILE_INK);
            let tx = x + 16.0 + 32.0 + 14.0;
            let mut ty = cy - text_h / 2.0;
            let g1h = g1.size().y.max(20.0);
            p.galley(pos2(tx, ty), g1, TEXT);
            ty += g1h;
            if let Some(m) = msg {
                ty += 2.0;
                let mh = m.size().y;
                p.galley(pos2(tx, ty), m, TEXT);
                ty += mh;
            }
            if let Some(g) = why {
                p.galley(pos2(tx, ty + 2.0), g, TEXT);
            }
            let busy = self.busy.contains(&format!("tip:{id}"));
            let show = Button::new("Show it").primary().sm().enabled(!busy);
            let refund = Button::new("Refund").sm().in_card(true).enabled(!busy);
            let (ss, rs) = (show.measure(ui), refund.measure(ui));
            let by = if narrow { r.bottom() - 12.0 - 30.0 } else { cy - 15.0 };
            let sr = Rect::from_min_size(pos2(r.right() - 16.0 - ss.x, by), ss);
            let rr = Rect::from_min_size(pos2(sr.left() - 8.0 - rs.x, by), rs);
            let approve = if show.show_at(ui, &self.icons, sr, Id::unique(("tip-show", &id))).clicked() {
                Some(true)
            } else if refund.show_at(ui, &self.icons, rr, Id::unique(("tip-refund", &id))).clicked() {
                Some(false)
            } else {
                None
            };
            if let Some(a) = approve {
                self.busy.insert(format!("tip:{id}"));
                let b = self.backend.clone();
                self.run(b.decide_tip(id.clone(), a));
            }
            y += h;
        }
        y - y0 + 24.0
    }

    fn totals(&mut self, ui: &mut egui::Ui, x: f32, y: f32, w: f32, full_w: f32) -> f32 {
        let narrow = full_w <= 640.0;
        let small = full_w <= 480.0;
        let gap = 12.0;
        let h = if small { 100.0 } else { 112.0 };
        let cards: [(&str, Option<String>, String, bool, String); 3] = [
            ("Viewers", self.viewers.loaded.then(|| self.viewers.count.clone()), self.viewers.foot.clone(), self.viewers.live, self.viewers.detail.clone()),
            ("Followers", self.followers.clone(), "All time".into(), false, String::new()),
            ("Subscribers", self.subs.clone(), "Active now".into(), false, String::new()),
        ];
        let col_w = if narrow { (w - gap) / 2.0 } else { (w - 2.0 * gap) / 3.0 };
        let mut rects = Vec::new();
        if narrow {
            rects.push(Rect::from_min_size(pos2(x, y), vec2(w, h)));
            rects.push(Rect::from_min_size(pos2(x, y + h + gap), vec2(col_w, h)));
            rects.push(Rect::from_min_size(pos2(x + col_w + gap, y + h + gap), vec2(col_w, h)));
        } else {
            for i in 0..3 {
                rects.push(Rect::from_min_size(pos2(x + i as f32 * (col_w + gap), y), vec2(col_w, h)));
            }
        }
        let p = ui.painter().clone();
        let pad = if small { 14.0 } else { 18.0 };
        for (i, (label, value, foot, live, detail)) in cards.into_iter().enumerate() {
            let r = rects[i];
            p.rect_filled(r, R_ITEM, SURFACE);
            let ix = r.left() + pad;
            let iw = r.width() - 2.0 * pad;
            let top = r.top() + if small { 14.0 } else { 16.0 };
            p.galley(pos2(ix, top), line(&p, label, regular(13.0), MUTED, iw), TEXT);
            let ny = top + 18.0 + 6.0;
            match value {
                Some(v) => {
                    let g = line(&p, v, medium(if small { 24.0 } else { 28.0 }), TEXT, iw);
                    p.galley(pos2(ix, ny + (34.0 - g.size().y) / 2.0), g, TEXT);
                }
                None => skel(ui, Rect::from_min_size(pos2(ix, ny + 4.0), vec2(if i == 1 { 96.0 } else { 72.0 }, 26.0))),
            }
            let fy = ny + 34.0 + 4.0;
            if i == 0 && !self.viewers.loaded {
                skel(ui, Rect::from_min_size(pos2(ix, fy + 3.5), vec2(60.0, 11.0)));
            } else {
                let color = if live { SAGE.gamma_multiply(breath(ui, 2.2)) } else { FAINT };
                let g = line(&p, foot, regular(12.5), color, iw);
                p.galley(pos2(ix, fy), g, TEXT);
                if !detail.is_empty() {
                    ui.interact(Rect::from_min_size(pos2(ix, fy), vec2(iw, 18.0)), Id::unique("viewer-detail"), Sense::hover()).on_hover_text(detail);
                }
            }
        }
        if narrow {
            2.0 * h + gap
        } else {
            h
        }
    }

    fn quick_actions(&mut self, ui: &mut egui::Ui, x: f32, y: f32, w: f32, full_w: f32) -> f32 {
        let one_col = full_w <= 640.0;
        let gap = 12.0;
        let h = 64.0;
        let col_w = if one_col { w } else { (w - gap) / 2.0 };
        let acts = [
            ("mute", self.muted, if self.muted { ("Unmute alerts", "Alerts are muted", "volx") } else { ("Mute alerts", "Alerts play with sound", "vol") }),
            ("pause", self.paused, if self.paused { ("Resume alerts", "Alerts wait in line", "play") } else { ("Pause alerts", "Alerts play as they come", "pause") }),
            ("skip", false, ("Skip alert", "End the one on screen", "skip")),
            ("reload", false, ("Reload overlays", "Refresh every OBS source", "layers")),
        ];
        let p = ui.painter().clone();
        for (i, (key, on, (t1, t2, icon))) in acts.into_iter().enumerate() {
            let (cx, cy) = if one_col { (x, y + i as f32 * (h + gap)) } else { (x + (i % 2) as f32 * (col_w + gap), y + (i / 2) as f32 * (h + gap)) };
            let r = Rect::from_min_size(pos2(cx, cy), vec2(col_w, h));
            let busy = self.busy.contains(&format!("qa:{key}"));
            let resp = ui.interact(r, Id::unique(("qa", key)), if busy { Sense::hover() } else { Sense::click() });
            let dim = if busy { 0.45 } else { 1.0 };
            p.rect_filled(r, R_ITEM, if resp.hovered() && !busy { ROW_HOVER } else { SURFACE });
            let tr = Rect::from_min_size(pos2(r.left() + 16.0, r.center().y - 16.0), vec2(32.0, 32.0));
            let t = ui.ctx().animate_bool_with_time(Id::unique(("qa-on", key)), on, 0.15);
            tile(&p, &self.icons, tr, icon, w::lerp(TILE, SAGE, t).gamma_multiply(dim), w::lerp(TILE_INK, ON_SAGE, t).gamma_multiply(dim));
            let tw = r.width() - 16.0 - 32.0 - 14.0 - 16.0;
            p.galley(pos2(tr.right() + 14.0, r.center().y - 20.0), line(&p, t1, medium(14.0), TEXT.gamma_multiply(dim), tw), TEXT);
            p.galley(pos2(tr.right() + 14.0, r.center().y + 2.0), line(&p, t2, regular(12.5), FAINT.gamma_multiply(dim), tw), TEXT);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(if busy { CursorIcon::Progress } else { CursorIcon::PointingHand });
            }
            if resp.clicked() {
                let (action, ok) = match key {
                    "mute" if on => ("unmute", "Alerts unmuted"),
                    "mute" => ("mute", "Alerts muted"),
                    "pause" if on => ("resume", "Alerts resumed"),
                    "pause" => ("pause", "Alerts paused"),
                    "skip" => ("skip", "Skipped the current alert"),
                    _ => ("reload", "Overlays reloaded"),
                };
                let k = format!("qa:{key}");
                self.busy.insert(k.clone());
                let b = self.backend.clone();
                self.run(b.alert_action(k, action, ok));
            }
        }
        if one_col {
            4.0 * h + 3.0 * gap
        } else {
            2.0 * h + gap
        }
    }

    /// Song requests: one compact row (what plays, play or pause, skip, the queue button), the
    /// position along the card's foot, and the queue folding out below. Audio only.
    fn song_requests(&mut self, ui: &mut egui::Ui, x: f32, y0: f32, w: f32) -> f32 {
        let Some(s) = self.sr.clone() else { return 0.0 };
        let mut y = y0;
        let on = s.view.settings.enabled;
        let p = ui.painter().clone();
        // Heading, with the on and off switch on the right.
        let label = line(&p, if on { "Taking requests" } else { "Requests off" }, regular(12.5), FAINT, 200.0);
        let right_w = label.size().x + 10.0 + 40.0;
        let head_h = self.sect_head(ui, x, y, w, "Song requests", "", right_w);
        let sw = Rect::from_min_size(pos2(x + w - 40.0, y + 5.0), vec2(40.0, 22.0));
        p.galley(pos2(sw.left() - 10.0 - label.size().x, sw.center().y - label.size().y / 2.0), label, TEXT);
        let busy = self.busy.contains("sr:enabled");
        if switch(ui, sw, Id::unique("sr-enabled"), on, "Take song requests").clicked() && !busy {
            self.busy.insert("sr:enabled".into());
            let b = self.backend.clone();
            self.run(b.sr_queue("enabled", None, None, Some(!on)));
        }
        y += head_h;

        let cur = s.view.current.clone();
        let queue = s.view.queue.clone();
        let total = if s.duration_ms > 0 { s.duration_ms } else { cur.as_ref().map_or(0, |c| c.duration_s as u64 * 1000) };
        let pos = self.sr_seek.unwrap_or(s.position_ms).min(total.max(1));
        let sub = if let Some(st) = &s.status {
            st.clone()
        } else if let Some(c) = &cur {
            if s.loading {
                format!("Loading · asked by {}", c.by)
            } else {
                format!("{} / {} · asked by {}", format::song_clock(pos), format::song_clock(total), c.by)
            }
        } else if on {
            "Viewers add songs with !sr".into()
        } else {
            "Turn requests on to take !sr in chat".into()
        };

        // Work out the height first so the card can be painted behind everything.
        let row_h = 64.0 + 8.0 - 12.0 + 12.0; // 12 top, 44 controls, 8 bottom
        let seek_h = 16.0;
        let list_rows = if self.sr_open { queue.len().max(1) } else { 0 };
        let list_h = if self.sr_open { (list_rows as f32 * 48.0).min(240.0) } else { 0.0 };
        let more_h = if self.sr_open { 8.0 + 40.0 + list_h + 8.0 } else { 0.0 };
        let card = Rect::from_min_size(pos2(x, y), vec2(w, row_h + seek_h + more_h));
        p.rect_filled(card, R_ITEM, SURFACE);

        let qlabel = format!("Queue {}", queue.len());
        let qbtn = Button::new(&qlabel).ghost().size(Size::Lg).tip(if self.sr_open { "Hide the queue" } else { "Show the queue" });
        let qs = qbtn.measure(ui);
        let controls_w = 44.0 + 12.0 + 44.0 + 12.0 + qs.x;
        let cy = y + 12.0 + 22.0;
        let tr = Rect::from_min_size(pos2(x + 16.0, cy - 16.0), vec2(32.0, 32.0));
        tile(&p, &self.icons, tr, "music", TILE, TILE_INK);
        let tw = w - 16.0 - 32.0 - 12.0 - 12.0 - controls_w - 16.0;
        let title = cur.as_ref().map_or("Nothing playing".to_string(), |c| c.title.clone());
        p.galley(pos2(tr.right() + 12.0, cy - 20.0), line(&p, title, medium(14.0), TEXT, tw), TEXT);
        p.galley(pos2(tr.right() + 12.0, cy + 2.0), line(&p, sub, regular(12.5), FAINT, tw), TEXT);

        let mut bx = x + w - 16.0 - controls_w;
        let has = cur.is_some();
        let play = Rect::from_min_size(pos2(bx, cy - 22.0), vec2(44.0, 44.0));
        bx += 56.0;
        let skip = Rect::from_min_size(pos2(bx, cy - 22.0), vec2(44.0, 44.0));
        bx += 56.0;
        if self.sr_button(ui, play, "sr-play", if s.playing { "pause" } else { "play" }, if s.playing { "Pause" } else { "Play" }, has) {
            let b = self.backend.clone();
            self.run(b.sr_control(if s.playing { SongControl::Pause } else { SongControl::Play }));
        }
        if self.sr_button(ui, skip, "sr-skip", "skip", "Skip", has) {
            let b = self.backend.clone();
            self.run(b.sr_queue("next", None, None, None));
        }
        if qbtn.show_at(ui, &self.icons, Rect::from_min_size(pos2(bx, cy - 22.0), qs), Id::unique("sr-queue")).clicked() {
            self.sr_open = !self.sr_open;
        }

        // The position, full width along the card's foot.
        let seek = Rect::from_min_size(pos2(x, y + row_h), vec2(w, seek_h));
        let can_seek = s.loaded.is_some() && total > 0;
        let resp = ui.interact(seek, Id::unique("sr-seek"), if can_seek { Sense::click_and_drag() } else { Sense::hover() });
        if can_seek && (resp.dragged() || resp.clicked()) {
            if let Some(px) = resp.interact_pointer_pos() {
                let f = ((px.x - seek.left()) / seek.width()).clamp(0.0, 1.0);
                self.sr_seek = Some((f as f64 * total as f64) as u64);
            }
        }
        if can_seek && (resp.drag_stopped() || resp.clicked()) {
            if let Some(v) = self.sr_seek {
                let b = self.backend.clone();
                self.run(b.sr_control(SongControl::Seek { position_ms: v }));
            }
        }
        let frac = if total > 0 { (self.sr_seek.unwrap_or(s.position_ms) as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
        let track = Rect::from_min_size(pos2(seek.left(), seek.center().y - 2.0), vec2(w, 4.0));
        let clip = p.with_clip_rect(card);
        clip.rect_filled(track, 0.0, SW_OFF);
        clip.rect_filled(Rect::from_min_size(track.min, vec2(w * frac, 4.0)), 0.0, SAGE);
        if can_seek && (resp.hovered() || resp.dragged()) {
            clip.circle_filled(pos2(track.left() + w * frac, track.center().y), 6.0, TEXT);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if self.sr_open {
            let mut yy = seek.bottom() + 8.0;
            // Volume.
            let vr = Rect::from_min_size(pos2(x + 16.0, yy), vec2(w - 32.0, 40.0));
            self.icons.paint(&p, "vol", pos2(vr.left() + 8.0, vr.center().y), 16.0, FAINT);
            let vol_txt = p.layout_no_wrap(format!("{}%", s.volume), mono(12.5), FAINT);
            p.galley(pos2(vr.right() - vol_txt.size().x, vr.center().y - vol_txt.size().y / 2.0), vol_txt, TEXT);
            let slider = Rect::from_min_max(pos2(vr.left() + 28.0, vr.center().y - 10.0), pos2(vr.right() - 52.0, vr.center().y + 10.0));
            let vresp = ui.interact(slider, Id::unique("sr-volume"), Sense::click_and_drag());
            if vresp.dragged() || vresp.clicked() {
                if let Some(px) = vresp.interact_pointer_pos() {
                    let f = ((px.x - slider.left()) / slider.width()).clamp(0.0, 1.0);
                    let v = ((f * 20.0).round() * 5.0) as u8;
                    if v != s.volume {
                        let b = self.backend.clone();
                        self.run(b.sr_control(SongControl::Volume { volume: v }));
                    }
                }
            }
            let vf = s.volume as f32 / 100.0;
            let vt = Rect::from_min_size(pos2(slider.left(), slider.center().y - 2.0), vec2(slider.width(), 4.0));
            p.rect_filled(vt, CornerRadius::same(2), SW_OFF);
            p.rect_filled(Rect::from_min_size(vt.min, vec2(slider.width() * vf, 4.0)), CornerRadius::same(2), SAGE);
            p.circle_filled(pos2(vt.left() + slider.width() * vf, vt.center().y), 7.0, TEXT);
            if vresp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            yy += 40.0;
            // The queue.
            let list = Rect::from_min_size(pos2(x, yy), vec2(w, list_h));
            if queue.is_empty() {
                p.galley(pos2(x + 16.0, yy + 8.0), line(&p, "No songs waiting.", regular(12.5), FAINT, w - 32.0), TEXT);
            } else {
                ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
                    egui::ScrollArea::vertical().id_salt("sr-list").max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
                        for (i, song) in queue.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
                            let lp = ui.painter().clone();
                            if resp.hovered() || resp.contains_pointer() {
                                lp.rect_filled(r, 0.0, ROW_HOVER);
                            }
                            let tw = w - 16.0 - 8.0 - 32.0 - 8.0 - 32.0 - 8.0;
                            lp.galley(pos2(r.left() + 16.0, r.center().y - 19.0), line(&lp, song.title.clone(), medium(14.0), TEXT, tw), TEXT);
                            lp.galley(pos2(r.left() + 16.0, r.center().y + 1.0), line(&lp, format!("{} · {}", song.by, format::song_clock(song.duration_s as u64 * 1000)), regular(12.5), FAINT, tw), TEXT);
                            let rm = Rect::from_min_size(pos2(r.right() - 8.0 - 32.0, r.center().y - 16.0), vec2(32.0, 32.0));
                            let up = Rect::from_min_size(pos2(rm.left() - 8.0 - 32.0, r.center().y - 16.0), vec2(32.0, 32.0));
                            if icon_button(ui, &self.icons, up, Id::unique(("sr-up", song.id)), "arrow-up", &format!("Play {} next", song.title), i > 0).clicked() {
                                let b = self.backend.clone();
                                self.run(b.sr_queue("move", Some(song.id), Some(0), None));
                            }
                            if icon_button(ui, &self.icons, rm, Id::unique(("sr-rm", song.id)), "trash", &format!("Remove {}", song.title), true).clicked() {
                                let b = self.backend.clone();
                                self.run(b.sr_queue("remove", Some(song.id), None, None));
                            }
                        }
                    });
                });
            }
        }
        y += card.height();
        y - y0
    }

    fn sr_button(&self, ui: &mut egui::Ui, r: Rect, id: &str, icon: &str, tip: &str, enabled: bool) -> bool {
        let resp = ui.interact(r, Id::unique(id), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
        let dim = if enabled { 1.0 } else { 0.45 };
        ui.painter().rect_filled(r, R_CONTROL, (if enabled && resp.hovered() { BTN_HOVER } else { BTN }).gamma_multiply(dim));
        self.icons.paint(ui.painter(), icon, r.center(), 16.0, TEXT.gamma_multiply(dim));
        w::hand(ui, &resp);
        resp.clicked()
    }

    // ---------- Activity ----------

    /// The card of activity rows: the latest few on Home, or one day's on Activity.
    fn feed_card(&mut self, ui: &mut egui::Ui, x: f32, y: f32, w: f32, scope: FeedScope, vis: Rect) -> f32 {
        let compact = self.prefs.compact;
        if !self.feed_loaded {
            let n = if matches!(scope, FeedScope::Recent) { 4 } else { 6 };
            let h = n as f32 * 60.0;
            ui.painter().rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, h)), R_ITEM, SURFACE);
            let widths = [0.58, 0.44, 0.66, 0.38, 0.52, 0.61];
            for i in 0..n {
                let ry = y + i as f32 * 60.0;
                skel(ui, Rect::from_min_size(pos2(x + 16.0, ry + 14.0), vec2(32.0, 32.0)));
                let lw = w - 16.0 - 32.0 - 14.0 - 16.0;
                skel(ui, Rect::from_min_size(pos2(x + 62.0, ry + 17.0), vec2(lw * widths[i % 6], 13.0)));
                skel(ui, Rect::from_min_size(pos2(x + 62.0, ry + 38.0), vec2(lw * 0.28, 10.0)));
            }
            return h;
        }
        let idx: Vec<usize> = match &scope {
            FeedScope::Recent => (0..self.feed.len().min(RECENT)).collect(),
            FeedScope::Day(v) => v.clone(),
        };
        if idx.is_empty() {
            if !matches!(scope, FeedScope::Recent) {
                return 0.0;
            }
            return self.empty_card(ui, x, y, w, "No activity yet. Follows, subs, bits and rewards appear here the moment they happen.");
        }
        // Measure every row, paint the card, then the rows (only those on screen).
        let p = ui.painter().clone();
        let laid: Vec<(usize, RowLayout)> = idx.iter().map(|&i| (i, layout_item(&p, &self.feed[i].a, w, compact))).collect();
        let total: f32 = laid.iter().map(|l| l.1.h).sum();
        p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, total)), R_ITEM, SURFACE);
        let now = ui.input(|i| i.time);
        let mut yy = y;
        let n = laid.len();
        for (k, (i, l)) in laid.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, yy), vec2(w, l.h));
            yy += l.h;
            if !r.intersects(vis) {
                continue;
            }
            let item = &self.feed[i];
            let a = item.a.clone();
            let fresh = ((now - item.born) / 0.2).clamp(0.0, 1.0) as f32;
            if fresh < 1.0 {
                ui.ctx().request_repaint();
            }
            let resp = ui.interact(r, Id::unique(("item", scope.key(), &a.id)), Sense::hover());
            let hovered = resp.contains_pointer();
            if hovered {
                let corners = CornerRadius { nw: if k == 0 { 12 } else { 0 }, ne: if k == 0 { 12 } else { 0 }, sw: if k == n - 1 { 12 } else { 0 }, se: if k == n - 1 { 12 } else { 0 } };
                p.rect_filled(r, corners, ROW_HOVER);
            }
            if let Some(id) = paint_item(ui, &self.icons, &a, l, r, hovered, fresh, compact, self.busy.contains(&format!("replay:{}", a.id))) {
                self.busy.insert(format!("replay:{id}"));
                let b = self.backend.clone();
                self.run(b.replay(id));
            }
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(30));
        total
    }

    fn empty_card(&self, ui: &egui::Ui, x: f32, y: f32, w: f32, text: &str) -> f32 {
        let p = ui.painter();
        let g = para(p, text, regular(13.5), FAINT, (w - 40.0).min(360.0), 0);
        let h = 36.0 + 20.0 + 8.0 + g.size().y + 36.0;
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        p.rect_filled(r, R_ITEM, SURFACE);
        self.icons.paint(p, "activity", pos2(r.center().x, y + 36.0 + 10.0), 20.0, FAINT);
        let gw = g.size().x;
        let gp = pos2(r.center().x - gw / 2.0, y + 36.0 + 28.0);
        // Center each line.
        let job_lines = g.clone();
        p.galley(gp, job_lines, TEXT);
        h
    }

    fn activity_page(&mut self, ui: &mut egui::Ui, x: f32, y0: f32, w: f32, vis: Rect) -> f32 {
        let mut y = y0;
        let btn = Button::new("Refresh").icon("refresh").spin(self.feed_loading).enabled(!self.feed_loading);
        let (h, refresh) = self.page_head(ui, x, y, w, "/activity", "Activity", "Everything that happened on your channel, newest first.", w < 452.0, Some(btn));
        if refresh {
            self.load_feed();
        }
        y += h;
        // Search, then the filter chips; they wrap below on narrow windows.
        let sw = (280.0f32).min(w);
        let sr = Rect::from_min_size(pos2(x, y), vec2(sw, 36.0));
        let before = self.query.clone();
        field(ui, sr.with_min_x(sr.left()), Id::unique("act-search"), &mut self.query, FieldOpts { hint: "Search names and messages", in_card: false, mono: false, multiline: false }.with_icon());
        self.icons.paint(ui.painter(), "search", pos2(sr.left() + 12.0 + 7.5, sr.center().y), 15.0, FAINT);
        if before != self.query {
            ui.ctx().request_repaint();
        }
        let mut cx = sr.right() + 12.0;
        let mut cy = y + 3.0;
        let mut row_bottom = y + 36.0;
        for (id, label) in chips() {
            let b = Button::new(label).sm().pressed(self.show == id);
            let s = b.measure(ui);
            if cx + s.x > x + w {
                cx = x;
                cy = row_bottom + 12.0;
                row_bottom = cy + s.y;
            }
            if b.show_at(ui, &self.icons, Rect::from_min_size(pos2(cx, cy), s), Id::unique(("chip", id))).clicked() {
                self.show = id.to_string();
            }
            cx += s.x + 6.0;
        }
        y = row_bottom.max(y + 36.0) + 18.0;

        if !self.feed_loaded {
            skel(ui, Rect::from_min_size(pos2(x + 4.0, y + 10.0), vec2(60.0, 12.0)));
            y += 40.0;
            y += self.feed_card(ui, x, y, w, FeedScope::Day(vec![]), vis);
            return y - y0;
        }
        let q = self.query.trim().to_lowercase();
        let items: Vec<usize> = (0..self.feed.len()).filter(|&i| activity::matches(&self.feed[i].a, &self.show, &q)).collect();
        if items.is_empty() {
            let text = if self.feed.is_empty() { "No activity yet. Follows, subs, bits and rewards appear here the moment they happen." } else { "Nothing here for this filter yet." };
            y += self.empty_card(ui, x, y, w, text);
            return y - y0;
        }
        // One section per day.
        let mut days: Vec<(String, String, Vec<usize>)> = Vec::new();
        for i in items {
            let at = self.feed[i].a.created_at.clone();
            let key = format::day_key(at.as_deref());
            match days.last_mut() {
                Some(d) if d.0 == key => d.2.push(i),
                _ => days.push((key, format::day_name(at.as_deref()), vec![i])),
            }
        }
        for (n, (key, name, idx)) in days.into_iter().enumerate() {
            if n > 0 {
                y += 20.0;
            }
            y += self.sect_head(ui, x, y, w, &name, "", 0.0);
            y += self.feed_card(ui, x, y, w, FeedScope::Day(idx), vis);
            let _ = key;
        }
        y - y0
    }

    // ---------- Stream info ----------

    fn stream_page(&mut self, ui: &mut egui::Ui, x: f32, y0: f32, w: f32) -> f32 {
        let mut y = y0;
        y += self.page_head(ui, x, y, w, "/stream", "Stream info", "Change your title and category on Twitch without opening the browser.", w < 452.0, None).0;
        let p = ui.painter().clone();
        if self.auth.twitch.is_none() {
            let btn = Button::new("Connect Twitch").primary();
            let bs = btn.measure(ui);
            let tw = w - 16.0 - 40.0 - 14.0 - bs.x - 14.0 - 16.0;
            let g2 = para(&p, "Connect once to change your title and category from here.", regular(12.5), FAINT, tw, 0);
            let h = (14.0 + 20.0 + 2.0 + g2.size().y + 14.0).max(64.0);
            let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
            p.rect_filled(r, R_ITEM, SURFACE);
            tile(&p, &self.icons, Rect::from_min_size(pos2(x + 16.0, r.center().y - 16.0), vec2(32.0, 32.0)), "twitch", TILE, TILE_INK);
            let ty = r.center().y - (22.0 + g2.size().y) / 2.0;
            p.galley(pos2(x + 62.0, ty), line(&p, "Connect Twitch", medium(14.0), TEXT, tw), TEXT);
            p.galley(pos2(x + 62.0, ty + 22.0), g2, TEXT);
            if btn.show_at(ui, &self.icons, Rect::from_min_size(pos2(r.right() - 16.0 - bs.x, r.center().y - bs.y / 2.0), bs), Id::unique("info-connect")).clicked() {
                self.open_twitch_dialog();
            }
            return y + h - y0;
        }
        let pad = if w < 452.0 { 14.0 } else { 18.0 };
        let iw = w - 2.0 * pad;
        // Title: grows with the text.
        let n = self.title_edit.chars().count();
        let title_g = para(&p, if self.title_edit.is_empty() { " ".to_string() } else { self.title_edit.clone() }, regular(14.5), TEXT, iw - 24.0, 0);
        let title_h = (title_g.size().y + 16.0).max(36.0);
        let form_h = pad + 18.0 + 8.0 + title_h + 16.0 + 60.0 + 16.0 + 36.0 + pad;
        let form = Rect::from_min_size(pos2(x, y), vec2(w, form_h));
        p.rect_filled(form, R_ITEM, SURFACE);
        let mut yy = y + pad;
        let fx = x + pad;
        p.text(pos2(fx, yy), Align2::LEFT_TOP, "Title", regular(13.0), MUTED);
        p.text(pos2(fx + iw, yy), Align2::RIGHT_TOP, format!("{n}/140"), mono(12.0), if n > 120 { WARN } else { FAINT });
        yy += 18.0 + 8.0;
        let tid = Id::unique("info-title");
        let enter = ui.ctx().memory(|m| m.has_focus(tid)) && ui.input(|i| i.key_pressed(Key::Enter));
        field(ui, Rect::from_min_size(pos2(fx, yy), vec2(iw, title_h)), tid, &mut self.title_edit, FieldOpts { hint: "Stream title", in_card: true, mono: false, multiline: true });
        // Titles are one line: Enter saves, and pasted line breaks become spaces.
        if self.title_edit.contains(['\r', '\n']) {
            self.title_edit = self.title_edit.replace("\r\n", " ").replace(['\r', '\n'], " ");
        }
        if self.title_edit.chars().count() > 140 {
            self.title_edit = self.title_edit.chars().take(140).collect();
        }
        if enter {
            self.save_info();
        }
        yy += title_h + 16.0;
        // Category: box art, then the search field with its list.
        let art = Rect::from_min_size(pos2(fx, yy), vec2(45.0, 60.0));
        self.box_art(ui, art, self.picked.as_ref());
        let cx = art.right() + 12.0;
        let cw = fx + iw - cx;
        p.text(pos2(cx, yy + 2.0), Align2::LEFT_TOP, "Category", regular(13.0), MUTED);
        let cat_rect = Rect::from_min_size(pos2(cx, yy + 24.0), vec2(cw, 36.0));
        let cid = Id::unique("info-cat");
        let before = self.cat_edit.clone();
        let had = self.cat_had_focus;
        // Up, Down and Enter move through the list instead of the text.
        let mut pick: Option<Category> = None;
        if had && self.cat_open && !self.cat_results.is_empty() {
            let (down, up, enter, esc) = ui.input_mut(|i| {
                (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape))
            });
            let len = self.cat_results.len();
            if down {
                self.cat_active = (self.cat_active + 1) % len;
            }
            if up {
                self.cat_active = (self.cat_active + len - 1) % len;
            }
            if enter {
                pick = self.cat_results.get(self.cat_active).cloned();
            }
            if esc {
                self.cat_open = false;
            }
        }
        let resp = field(ui, cat_rect, cid, &mut self.cat_edit, FieldOpts { hint: "Search categories", in_card: true, mono: false, multiline: false });
        let focused = resp.has_focus();
        if focused && !had {
            // Clicking in selects it all, so typing starts a new search.
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), cid) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.cat_edit.chars().count()))));
                st.store(ui.ctx(), cid);
            }
        }
        if self.cat_edit != before {
            // Typing clears the earlier pick until one is chosen from the list.
            self.picked = None;
            if self.cat_edit.trim().is_empty() {
                self.cat_open = false;
                self.cat_due = None;
            } else {
                self.cat_due = Some(ui.input(|i| i.time) + 0.25);
            }
        }
        // The list, under the field.
        if focused && self.cat_open {
            if let Some(c) = self.category_list(ui, cat_rect) {
                pick = Some(c);
            }
        }
        let picked_now = pick.is_some();
        if let Some(c) = pick {
            self.cat_edit = c.name.clone();
            self.picked = Some(c);
            self.cat_open = false;
            ui.ctx().memory_mut(|m| m.surrender_focus(cid));
        }
        if had && !focused && !picked_now {
            // Leaving the box without picking puts the current category back.
            self.cat_open = false;
            self.cat_due = None;
            self.cat_req += 1;
            if self.picked.is_none() {
                if let Some(c) = self.channel.clone() {
                    self.cat_edit = c.game_name.clone();
                    self.picked = (!c.game_id.is_empty()).then(|| Category { id: c.game_id, name: c.game_name, box_art_url: String::new() });
                }
            }
        }
        self.cat_had_focus = focused && !picked_now;
        yy += 60.0 + 16.0;
        // Foot: what state the fields are in, and Save.
        let changed = self.info_changed();
        let state = if self.saving {
            "Saving"
        } else if self.channel.is_none() {
            "Reading from Twitch"
        } else if changed {
            "Not saved yet"
        } else {
            "Saved on Twitch"
        };
        let save = Button::new("Save").primary().enabled(changed && !self.saving);
        let ss = vec2(save.measure(ui).x.max(88.0), 36.0);
        let sr = Rect::from_min_size(pos2(fx + iw - ss.x, yy), ss);
        let sg = p.layout_no_wrap(state.to_string(), regular(12.5), FAINT);
        p.galley(pos2(sr.left() - 12.0 - sg.size().x, sr.center().y - sg.size().y / 2.0), sg, TEXT);
        if save.width(ss.x).show_at(ui, &self.icons, sr, Id::unique("info-save")).clicked() {
            self.save_info();
        }
        y += form_h;
        y - y0
    }

    fn box_art(&self, ui: &egui::Ui, r: Rect, c: Option<&Category>) {
        let p = ui.painter();
        p.rect_filled(r, R_CONTROL, BTN_IN);
        self.icons.paint(p, "image", r.center(), 16.0, FAINT);
        let Some(url) = c.and_then(box_art_url) else { return };
        if let Some(a) = self.images.get(&url) {
            let (tex, _) = a.frame(0);
            let mut mesh = egui::Mesh::with_texture(tex.id());
            mesh.add_rect_with_uv(r, w::cover_uv(r, a.size), Color32::WHITE);
            p.rect_filled(r, R_CONTROL, BTN_IN);
            p.with_clip_rect(r).add(mesh);
        }
    }

    fn category_list(&mut self, ui: &mut egui::Ui, field_rect: Rect) -> Option<Category> {
        let mut picked = None;
        let pos = pos2(field_rect.left(), field_rect.bottom() + 6.0);
        let results = self.cat_results.clone();
        egui::Area::new(Id::unique("cat-list")).order(Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            w::popup_frame(12, 6).show(ui, |ui| {
                ui.set_width(field_rect.width() - 12.0);
                if results.is_empty() {
                    let (r, _) = ui.allocate_exact_size(vec2(field_rect.width() - 12.0, 36.0), Sense::hover());
                    ui.painter().text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, "No categories found", regular(13.5), FAINT);
                    return;
                }
                egui::ScrollArea::vertical().max_height(308.0).show(ui, |ui| {
                    for (i, c) in results.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(field_rect.width() - 12.0, 52.0), Sense::click());
                        if i == self.cat_active || resp.hovered() {
                            ui.painter().rect_filled(r, R_CONTROL, BTN_IN);
                        }
                        let art = Rect::from_min_size(pos2(r.left() + 8.0, r.center().y - 20.0), vec2(30.0, 40.0));
                        self.box_art(ui, art, Some(c));
                        let g = line(ui.painter(), c.name.clone(), regular(13.5), TEXT, r.width() - 60.0);
                        ui.painter().galley(pos2(art.right() + 12.0, r.center().y - g.size().y / 2.0), g, TEXT);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        // Pick on press, before the field loses focus.
                        if resp.is_pointer_button_down_on() || resp.clicked() {
                            picked = Some(c.clone());
                        }
                    }
                });
            });
        });
        picked
    }

    // ---------- Signed out ----------

    fn welcome(&mut self, ui: &mut egui::Ui, panel: Rect) {
        let p = ui.painter().clone();
        p.rect_filled(panel, CornerRadius::same(R_PANEL), BG);
        let cw = (panel.width() - 40.0).min(400.0);
        let body = para(&p, "Your activity, alerts and stream info in one small window next to OBS. Sign in and approve Sylva Control in the browser that opens.", regular(14.0), MUTED, cw, 0);
        let code = self.signin_code.as_ref().map(|c| para(&p, c, regular(12.5), FAINT, cw, 0));
        let err = self.signin_error.as_ref().map(|e| para(&p, e, regular(13.0), DANGER, cw, 0));
        let foot = para(&p, "Your sign-in stays in your system's credential store, never in a file.", regular(12.5), FAINT, cw, 0);
        let title = para(&p, "Sign in to Sylva Control", medium(24.0), TEXT, cw, 0);
        let gap = 12.0;
        let mut h = 56.0 + 8.0 + gap + 26.0 + gap + 4.0 + title.size().y + gap + body.size().y + gap + 8.0 + 44.0 + gap + foot.size().y;
        h += code.as_ref().map_or(0.0, |g| g.size().y + gap) + err.as_ref().map_or(0.0, |g| g.size().y + gap);
        let cx = panel.center().x;
        let mut y = panel.center().y - h / 2.0;
        // A gentle 6px float on the logo.
        let t = ui.input(|i| i.time);
        let float = if ui.ctx().options(|o| o.dark_style.animation_time) > 0.0 { ((t * std::f64::consts::TAU / 2.2).sin() * 0.5 + 0.5) as f32 * -6.0 } else { 0.0 };
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        let logo = Rect::from_center_size(pos2(cx, y + 28.0 + float), vec2(56.0, 56.0));
        p.rect_filled(logo, R_ITEM, SURFACE);
        p.image(self.icons.get("leaf").id(), Rect::from_center_size(logo.center(), vec2(22.0, 31.0)), uv(), Color32::WHITE);
        y += 56.0 + 8.0 + gap;
        let tg = p.layout_no_wrap("companion app".into(), mono(13.0), SAGE);
        let tag = Rect::from_center_size(pos2(cx, y + 13.0), vec2(tg.size().x + 20.0, 26.0));
        p.rect_filled(tag, R_CONTROL, TAG_BG);
        p.galley(tag.center() - tg.size() / 2.0, tg, SAGE);
        y += 26.0 + gap + 4.0;
        let center = |g: Arc<egui::Galley>, y: &mut f32| {
            let s = g.size();
            p.galley(pos2(cx - s.x / 2.0, *y), g, TEXT);
            *y += s.y + gap;
        };
        center(title, &mut y);
        center(body, &mut y);
        if let Some(g) = code {
            center(g, &mut y);
        }
        if let Some(g) = err {
            center(g, &mut y);
        }
        y += 8.0;
        let label = if self.signin_busy { "Waiting for the browser..." } else { "Sign in with Sylva" };
        let b = Button::new(label).primary().size(Size::Lg).width(cw).enabled(!self.signin_busy);
        if b.show_at(ui, &self.icons, Rect::from_min_size(pos2(cx - cw / 2.0, y), vec2(cw, 44.0)), Id::unique("signin")).clicked() {
            self.signin_busy = true;
            self.signin_error = None;
            let b = self.backend.clone();
            self.run(b.sylva_sign_in());
        }
        y += 44.0 + gap;
        center(foot, &mut y);
    }

    // ---------- Settings and dialogs ----------

    /// The dimmed backdrop under a window. It starts under the title bar so the app can always
    /// be moved, minimized or closed. Returns true when it was clicked.
    fn scrim(&self, ctx: &egui::Context, id: &str) -> bool {
        let screen = ctx.content_rect();
        let r = Rect::from_min_max(pos2(screen.left(), screen.top() + TITLEBAR), screen.max);
        let mut clicked = false;
        egui::Area::new(Id::unique(("scrim", id))).order(Order::Middle).fixed_pos(r.min).show(ctx, |ui| {
            let resp = ui.allocate_rect(Rect::from_min_size(r.min, r.size()), Sense::click());
            ui.painter().rect_filled(r, 0.0, SCRIM);
            clicked = resp.clicked();
        });
        clicked
    }

    fn settings_modal(&mut self, ctx: &egui::Context) {
        let Some(pane) = self.settings else { return };
        if self.scrim(ctx, "settings") || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.settings = None;
            return;
        }
        let screen = ctx.content_rect();
        let avail = Rect::from_min_max(pos2(screen.left() + 16.0, screen.top() + TITLEBAR + 16.0), pos2(screen.right() - 16.0, screen.bottom() - 16.0));
        let stacked = screen.width() <= 640.0;
        let size = vec2(avail.width().min(760.0), avail.height().min(if stacked { 560.0 } else { 500.0 }));
        let rect = Rect::from_center_size(avail.center(), size);
        egui::Area::new(Id::unique("settings")).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            let p = ui.painter().clone();
            ui.allocate_rect(rect, Sense::click()); // keeps clicks inside from reaching the backdrop
            p.add(w::shadow().as_shape(rect, CornerRadius::same(R_PANEL)));
            p.rect_filled(rect, CornerRadius::same(R_PANEL), DRAWER);
            // Sections: a column on the left, or a row on top when narrow.
            let (side, main) = if stacked {
                let side = Rect::from_min_size(rect.min, vec2(rect.width(), 12.0 + 30.0 + 36.0 + 12.0));
                (side, Rect::from_min_max(pos2(rect.left(), side.bottom()), rect.max))
            } else {
                let side = Rect::from_min_size(rect.min, vec2(210.0, rect.height()));
                (side, Rect::from_min_max(pos2(side.right(), rect.top()), rect.max))
            };
            let side_corners = if stacked { CornerRadius { nw: R_PANEL, ne: R_PANEL, ..Default::default() } } else { CornerRadius { nw: R_PANEL, sw: R_PANEL, ..Default::default() } };
            p.rect_filled(side, side_corners, BG);
            let sx = side.left() + 12.0;
            let mut sy = side.top() + if stacked { 12.0 } else { 16.0 };
            p.text(pos2(sx + 10.0, sy + 4.0), Align2::LEFT_TOP, "Settings", semibold(15.0), TEXT);
            sy += if stacked { 30.0 } else { 34.0 };
            let mut nx = sx;
            for (pn, icon) in [(Pane::Accounts, "user"), (Pane::Display, "monitor"), (Pane::About, "info")] {
                let r = if stacked {
                    let g = ui.painter().layout_no_wrap(pn.title().into(), medium(14.0), TEXT);
                    let r = Rect::from_min_size(pos2(nx, sy), vec2(g.size().x + 24.0 + 27.0, 36.0));
                    nx += r.width() + 2.0;
                    r
                } else {
                    let r = Rect::from_min_size(pos2(sx, sy), vec2(side.width() - 24.0, 36.0));
                    sy += 38.0;
                    r
                };
                if self.side_item(ui, r, icon, pn.title(), pane == pn, false).clicked() {
                    self.settings = Some(pn);
                }
            }
            // Heading and close.
            let hx = main.left() + if stacked { 16.0 } else { 24.0 };
            let hy = main.top() + if stacked { 12.0 } else { 18.0 };
            p.text(pos2(hx, hy + 13.0), Align2::LEFT_CENTER, pane.title(), medium(20.0), TEXT);
            let close = Rect::from_min_size(pos2(main.right() - 20.0 - 32.0, hy - 3.0), vec2(32.0, 32.0));
            if icon_button(ui, &self.icons, close, Id::unique("settings-close"), "x", "Close settings", true).clicked() {
                self.settings = None;
            }
            let body = Rect::from_min_max(pos2(hx, hy + 26.0 + 6.0 + 14.0), pos2(main.right() - if stacked { 16.0 } else { 24.0 }, main.bottom() - 24.0));
            ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::vertical().id_salt("settings-body").auto_shrink([false, false]).show(ui, |ui| {
                    let bw = body.width();
                    let origin = ui.min_rect().min;
                    let h = match pane {
                        Pane::Accounts => self.pane_accounts(ui, origin, bw),
                        Pane::Display => self.pane_display(ui, origin, bw),
                        Pane::About => self.pane_about(ui, origin, bw),
                    };
                    ui.allocate_space(vec2(bw, h));
                });
            });
        });
    }

    /// A settings row: optional tile, two lines of text, and room on the right for controls.
    fn set_row(&self, ui: &egui::Ui, r: Rect, icon: Option<&str>, t1: &str, t2: &str, ctl_w: f32, mono2: bool) {
        let p = ui.painter();
        let mut tx = r.left() + 16.0;
        if let Some(i) = icon {
            tile(p, &self.icons, Rect::from_min_size(pos2(tx, r.center().y - 16.0), vec2(32.0, 32.0)), i, TILE, TILE_INK);
            tx += 32.0 + 14.0;
        }
        let tw = r.right() - 16.0 - ctl_w - 14.0 - tx;
        p.galley(pos2(tx, r.center().y - 20.0), line(p, t1, medium(14.0), TEXT, tw), TEXT);
        p.galley(pos2(tx, r.center().y + 2.0), line(p, t2, if mono2 { mono(12.5) } else { regular(12.5) }, FAINT, tw), TEXT);
    }

    fn sect_foot(&self, ui: &egui::Ui, x: f32, y: f32, w: f32, text: &str) -> f32 {
        let g = para(ui.painter(), text, regular(12.5), FAINT, w - 8.0, 0);
        let h = g.size().y;
        ui.painter().galley(pos2(x + 4.0, y + 10.0), g, TEXT);
        10.0 + h
    }

    fn pane_accounts(&mut self, ui: &mut egui::Ui, o: Pos2, w: f32) -> f32 {
        let p = ui.painter().clone();
        let card = Rect::from_min_size(o, vec2(w, 120.0));
        p.rect_filled(card, R_ITEM, SURFACE);
        let r1 = Rect::from_min_size(o, vec2(w, 60.0));
        let r2 = Rect::from_min_size(o + vec2(0.0, 60.0), vec2(w, 60.0));
        let sy_name = self.auth.sylva.as_ref().map(|s| s.name.clone().or(s.login.clone()).unwrap_or_else(|| "Streamer".into()));
        let out = Button::new("Sign out").danger().sm().in_card(true);
        let ow = if sy_name.is_some() { out.measure(ui).x } else { 0.0 };
        self.set_row(ui, r1, Some("leaf"), "Sylva", &sy_name.as_ref().map_or("Not signed in".into(), |n| format!("Signed in as {n}")), ow, false);
        if sy_name.is_some() && out.show_at(ui, &self.icons, Rect::from_min_size(pos2(r1.right() - 16.0 - ow, r1.center().y - 15.0), vec2(ow, 30.0)), Id::unique("sy-signout")).clicked() {
            self.settings = None;
            let b = self.backend.clone();
            self.run(b.sylva_sign_out());
        }
        let connected = self.auth.twitch.clone();
        let tb = if connected.is_some() { Button::new("Disconnect") } else { Button::new("Connect") }.sm().in_card(true);
        let tw = tb.measure(ui).x;
        self.set_row(ui, r2, Some("twitch"), "Twitch", &connected.as_ref().map_or("Not connected".into(), |l| format!("Connected as {l}")), tw, false);
        if tb.show_at(ui, &self.icons, Rect::from_min_size(pos2(r2.right() - 16.0 - tw, r2.center().y - 15.0), vec2(tw, 30.0)), Id::unique("tw-account")).clicked() {
            if connected.is_some() {
                let b = self.backend.clone();
                self.run(b.twitch_sign_out());
            } else {
                self.open_twitch_dialog();
            }
        }
        120.0 + self.sect_foot(ui, o.x, o.y + 120.0, w, "Twitch lets Sylva Control change your title and category, read your viewer count straight from Twitch and count down to ad breaks.")
    }

    fn pane_display(&mut self, ui: &mut egui::Ui, o: Pos2, w: f32) -> f32 {
        let p = ui.painter().clone();
        p.rect_filled(Rect::from_min_size(o, vec2(w, 180.0)), R_ITEM, SURFACE);
        let rows = [
            ("adbar", "Ad break countdown", "Time left until the next Twitch ad break, on Home"),
            ("actions", "Quick actions", "Mute, pause and skip alerts from Home"),
            ("compact", "Compact rows", "Smaller activity rows, so more fit on screen"),
        ];
        for (i, (k, t1, t2)) in rows.into_iter().enumerate() {
            let r = Rect::from_min_size(o + vec2(0.0, i as f32 * 60.0), vec2(w, 60.0));
            self.set_row(ui, r, None, t1, t2, 40.0, false);
            let on = match k {
                "adbar" => self.prefs.adbar,
                "actions" => self.prefs.actions,
                _ => self.prefs.compact,
            };
            if switch(ui, Rect::from_min_size(pos2(r.right() - 16.0 - 40.0, r.center().y - 11.0), vec2(40.0, 22.0)), Id::unique(("pref", k)), on, t1).clicked() {
                match k {
                    "adbar" => self.prefs.adbar = !on,
                    "actions" => self.prefs.actions = !on,
                    _ => self.prefs.compact = !on,
                }
            }
        }
        180.0
    }

    fn pane_about(&mut self, ui: &mut egui::Ui, o: Pos2, w: f32) -> f32 {
        let p = ui.painter().clone();
        let r = Rect::from_min_size(o, vec2(w, 60.0));
        p.rect_filled(r, R_ITEM, SURFACE);
        let b = Button::new("Check for updates").sm().in_card(true);
        let bw = if self.update_supported { b.measure(ui).x } else { 0.0 };
        self.set_row(ui, r, None, "Version", env!("CARGO_PKG_VERSION"), bw, true);
        if self.update_supported && b.show_at(ui, &self.icons, Rect::from_min_size(pos2(r.right() - 16.0 - bw, r.center().y - 15.0), vec2(bw, 30.0)), Id::unique("check-update")).clicked() {
            self.run(updater::check(self.backend.notify.clone(), true));
        }
        60.0 + self.sect_foot(ui, o.x, o.y + 60.0, w, "Sylva Control checks for updates on its own every 15 minutes.")
    }

    fn twitch_dialog(&mut self, ctx: &egui::Context) {
        if !self.twitch_dialog {
            return;
        }
        if self.scrim(ctx, "twitch") || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.close_twitch_dialog();
            return;
        }
        let screen = ctx.content_rect();
        let dw = (screen.width() - 32.0).min(440.0);
        let iw = dw - 48.0;
        let fonts_p = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("tw-measure")));
        let intro = para(&fonts_p, "Connecting lets Sylva Control change your stream title and category, read your viewer count straight from Twitch, and warn you before an ad break. You need a free Twitch app Client ID, once.", regular(13.5), MUTED, iw, 0);
        let steps: [Vec<(String, bool, bool)>; 3] = [
            vec![("Register an app at ".into(), false, false), ("dev.twitch.tv/console/apps".into(), false, true), (".".into(), false, false)],
            vec![("Set Client Type to ".into(), false, false), ("Public".into(), true, false), (". For the redirect URL, use http://localhost.".into(), false, false)],
            vec![("Paste the Client ID below.".into(), false, false)],
        ];
        let step_g: Vec<Arc<egui::Galley>> = steps
            .iter()
            .map(|parts| {
                let mut job = LayoutJob::default();
                for (t, bold, link) in parts {
                    let color = if *link { SAGE } else if *bold { TEXT } else { MUTED };
                    job.append(t, 0.0, fmt(if *bold { medium(13.5) } else { regular(13.5) }, color));
                }
                job.wrap.max_width = iw - 30.0;
                fonts_p.layout_job(job)
            })
            .collect();
        let code_h = if self.tw_code.is_some() { 16.0 + 20.0 + 8.0 + 32.0 + 8.0 + 20.0 + 16.0 + 12.0 } else { 0.0 };
        let steps_h: f32 = step_g.iter().map(|g| g.size().y.max(20.0)).sum::<f32>() + 16.0;
        let h = 24.0 + 40.0 + 12.0 + 26.0 + 12.0 + intro.size().y + 12.0 + steps_h + 12.0 + 18.0 + 8.0 + 36.0 + 12.0 + 40.0 + code_h + 24.0;
        let rect = Rect::from_center_size(pos2(screen.center().x, (screen.top() + TITLEBAR + screen.bottom()) / 2.0), vec2(dw, h.min(screen.height() - TITLEBAR - 32.0)));
        egui::Area::new(Id::unique("twitch-dialog")).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            let p = ui.painter().clone();
            ui.allocate_rect(rect, Sense::click());
            p.add(w::shadow().as_shape(rect, CornerRadius::same(R_PANEL)));
            p.rect_filled(rect, CornerRadius::same(R_PANEL), DRAWER);
            let x = rect.left() + 24.0;
            let mut y = rect.top() + 24.0;
            tile(&p, &self.icons, Rect::from_min_size(pos2(x, y), vec2(40.0, 40.0)), "twitch", TILE, TILE_INK);
            if icon_button(ui, &self.icons, Rect::from_min_size(pos2(rect.right() - 24.0 - 32.0, y + 4.0), vec2(32.0, 32.0)), Id::unique("twitch-close"), "x", "Close", true).clicked() {
                self.close_twitch_dialog();
            }
            y += 40.0 + 12.0;
            p.text(pos2(x, y + 13.0), Align2::LEFT_CENTER, "Connect Twitch", medium(20.0), TEXT);
            y += 26.0 + 12.0;
            let ih = intro.size().y;
            p.galley(pos2(x, y), intro, TEXT);
            y += ih + 12.0;
            for (i, g) in step_g.into_iter().enumerate() {
                let n = Rect::from_min_size(pos2(x, y), vec2(20.0, 20.0));
                p.rect_filled(n, R_CONTROL, TAG_BG);
                p.text(n.center(), Align2::CENTER_CENTER, (i + 1).to_string(), mono(11.5), SAGE);
                let gh = g.size().y.max(20.0);
                let gr = Rect::from_min_size(pos2(x + 30.0, y + 1.0), g.size());
                p.galley(gr.min, g, TEXT);
                if i == 0 {
                    // The link in step 1.
                    let resp = ui.interact(gr, Id::unique("tw-console-link"), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        let _ = open::that_detached("https://dev.twitch.tv/console/apps");
                    }
                }
                y += gh + 8.0;
            }
            y += 4.0;
            p.text(pos2(x, y), Align2::LEFT_TOP, "Client ID", regular(13.0), MUTED);
            y += 18.0 + 8.0;
            field(ui, Rect::from_min_size(pos2(x, y), vec2(iw, 36.0)), Id::unique("tw-client-id"), &mut self.twitch_client_id, FieldOpts { hint: "a1b2c3d4...", in_card: true, mono: true, multiline: false });
            y += 36.0 + 12.0;
            let b = Button::new("Continue on Twitch").primary().size(Size::Dialog).width(iw).enabled(!self.tw_busy);
            if b.show_at(ui, &self.icons, Rect::from_min_size(pos2(x, y), vec2(iw, 40.0)), Id::unique("tw-connect")).clicked() {
                self.tw_busy = true;
                let b = self.backend.clone();
                let id = self.twitch_client_id.trim().to_string();
                self.run(b.twitch_sign_in(id));
            }
            y += 40.0 + 12.0;
            if let Some(code) = self.tw_code.clone() {
                let box_r = Rect::from_min_size(pos2(x, y), vec2(iw, code_h - 12.0));
                p.rect_filled(box_r, R_ITEM, SURFACE);
                let mut yy = y + 16.0;
                p.text(pos2(x + 16.0, yy), Align2::LEFT_TOP, "Enter this code on the Twitch page that opened:", regular(13.5), MUTED);
                yy += 20.0 + 8.0;
                let spaced: String = code.user_code.chars().map(|c| c.to_string()).collect::<Vec<_>>().join("\u{2009}");
                p.text(pos2(box_r.center().x, yy + 16.0), Align2::CENTER_CENTER, spaced, mono(26.0), TEXT);
                yy += 32.0 + 8.0;
                let g1 = p.layout_no_wrap("Waiting for you to approve. ".into(), regular(12.5), FAINT);
                let g2 = p.layout_no_wrap("Open the page again".into(), regular(12.5), SAGE);
                let g3 = p.layout_no_wrap(" or ".into(), regular(12.5), FAINT);
                let g4 = p.layout_no_wrap("cancel".into(), regular(12.5), SAGE);
                let g5 = p.layout_no_wrap(".".into(), regular(12.5), FAINT);
                let mut lx = x + 16.0;
                for (g, link) in [(g1, 0), (g2, 1), (g3, 0), (g4, 2), (g5, 0)] {
                    let r = Rect::from_min_size(pos2(lx, yy), g.size());
                    lx += g.size().x;
                    p.galley(r.min, g, TEXT);
                    if link > 0 {
                        let resp = ui.interact(r, Id::unique(("tw-link", link)), Sense::click());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            if link == 1 {
                                let _ = open::that_detached(&code.verification_uri);
                            } else {
                                self.tw_code = None;
                                self.tw_busy = false;
                                let b = self.backend.clone();
                                self.run(b.twitch_cancel());
                            }
                        }
                    }
                }
            }
        });
    }

    fn update_dialog(&mut self, ctx: &egui::Context) {
        let Some((step, text)) = self.update_dialog.clone() else { return };
        self.scrim(ctx, "update");
        let screen = ctx.content_rect();
        let dw = (screen.width() - 32.0).min(440.0);
        let iw = dw - 48.0;
        let failed = step == "error";
        let mp = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("upd-measure")));
        let step_text = if failed { "The update did not finish. Sylva Control is still on the version you had.".to_string() } else { text.clone() };
        let sg = para(&mp, step_text, regular(13.5), MUTED, iw, 0);
        let hint = para(&mp, "Sylva Control restarts by itself when it is done. You can keep it open.", regular(12.5), FAINT, iw, 0);
        let err = failed.then(|| para(&mp, text.clone(), mono(11.5), MUTED, iw - 24.0, 12));
        let h = 24.0 + 26.0 + 12.0 + sg.size().y + 12.0 + if failed { err.as_ref().unwrap().size().y + 20.0 + 12.0 + 40.0 } else { 4.0 + 12.0 + hint.size().y } + 24.0;
        let rect = Rect::from_center_size(pos2(screen.center().x, (screen.top() + TITLEBAR + screen.bottom()) / 2.0), vec2(dw, h));
        egui::Area::new(Id::unique("update-dialog")).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            let p = ui.painter().clone();
            ui.allocate_rect(rect, Sense::click());
            p.add(w::shadow().as_shape(rect, CornerRadius::same(R_PANEL)));
            p.rect_filled(rect, CornerRadius::same(R_PANEL), DRAWER);
            let x = rect.left() + 24.0;
            let mut y = rect.top() + 24.0;
            p.text(pos2(x, y + 13.0), Align2::LEFT_CENTER, "Updating Sylva Control", medium(20.0), TEXT);
            y += 26.0 + 12.0;
            let sh = sg.size().y;
            p.galley(pos2(x, y), sg, TEXT);
            y += sh + 12.0;
            if failed {
                let e = err.unwrap();
                let er = Rect::from_min_size(pos2(x, y), vec2(iw, e.size().y + 20.0));
                p.rect_filled(er, R_ITEM, SURFACE);
                p.galley(pos2(x + 12.0, y + 10.0), e, TEXT);
                y += er.height() + 12.0;
                if Button::new("Close").size(Size::Dialog).show_at(ui, &self.icons, Rect::from_min_size(pos2(x, y), vec2(90.0, 40.0)), Id::unique("update-close")).clicked() {
                    self.update_dialog = None;
                }
            } else {
                // A bar that slides along while it works.
                let track = Rect::from_min_size(pos2(x, y), vec2(iw, 4.0));
                p.rect_filled(track, CornerRadius::same(2), SURFACE_3);
                let t = (ui.input(|i| i.time) / 1.4).fract() as f32;
                let seg = iw * 0.35;
                let sx = track.left() - seg + t * (iw + seg) * 1.0;
                p.with_clip_rect(track).rect_filled(Rect::from_min_size(pos2(sx, y), vec2(seg, 4.0)), CornerRadius::same(2), SAGE);
                ui.ctx().request_repaint();
                y += 4.0 + 12.0;
                p.galley(pos2(x, y), hint, TEXT);
            }
        });
    }
}

fn wall_s() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn full_narrow(w: f32) -> bool {
    w < 452.0
}

/// A slow breath between 0.55 and 1, for things that are live.
fn breath(ui: &egui::Ui, period: f64) -> f32 {
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    let t = ui.input(|i| i.time);
    0.775 + 0.225 * ((t * std::f64::consts::TAU / period).cos() as f32)
}

fn box_art_url(c: &Category) -> Option<String> {
    if c.id.is_empty() {
        return None;
    }
    let url = if c.box_art_url.is_empty() { format!("https://static-cdn.jtvnw.net/ttv-boxart/{}-{{width}}x{{height}}.jpg", c.id) } else { c.box_art_url.clone() };
    let url = url.replace("{width}x{height}", "72x96");
    // Links that already carry a size get the picker's size instead.
    let re_end = url.rfind('-').and_then(|i| {
        let tail = &url[i + 1..];
        let dot = tail.find('.')?;
        let (dims, ext) = tail.split_at(dot);
        let (a, b) = dims.split_once('x')?;
        (a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit())).then(|| format!("{}-72x96{}", &url[..i], ext))
    });
    Some(re_end.unwrap_or(url))
}

enum FeedScope {
    Recent,
    Day(Vec<usize>),
}

impl FeedScope {
    fn key(&self) -> &'static str {
        match self {
            FeedScope::Recent => "recent",
            FeedScope::Day(_) => "day",
        }
    }
}

struct RowLayout {
    h: f32,
    title: Arc<egui::Galley>,
    msg: Option<Arc<egui::Galley>>,
    pills: Vec<(Arc<egui::Galley>, bool)>,
    time: Arc<egui::Galley>,
    text_h: f32,
}

fn layout_item(p: &egui::Painter, a: &Activity, w: f32, compact: bool) -> RowLayout {
    let d = describe(a);
    let tile_w = if compact { 26.0 } else { 32.0 };
    let side_w = 64.0 + 8.0 + 30.0;
    let tw = (w - 16.0 - tile_w - 14.0 - 14.0 - side_w - 16.0).max(60.0);
    let mut job = LayoutJob::default();
    for (t, bold) in &d.parts {
        job.append(t, 0.0, fmt(if *bold { semibold(14.0) } else { regular(14.0) }, if *bold { TEXT } else { TEXT_2 }));
    }
    job.wrap.max_width = tw;
    let title = p.layout_job(job);
    let msg = d.message.as_ref().map(|m| para(p, m.clone(), regular(12.5), FAINT, tw, 2));
    let mut pills: Vec<(Arc<egui::Galley>, bool)> = d.pills.iter().map(|t| (p.layout_no_wrap(t.clone(), regular(11.5), MUTED), false)).collect();
    if a.test {
        pills.push((p.layout_no_wrap("test".into(), mono(11.5), SAGE), true));
    }
    let mut text_h = title.size().y.max(20.0);
    if let Some(m) = &msg {
        text_h += 2.0 + m.size().y;
    }
    if !pills.is_empty() {
        // Pills wrap onto more rows when they do not fit.
        let mut rows = 1;
        let mut x = 0.0;
        for (g, _) in &pills {
            let pw = g.size().x + 14.0;
            if x > 0.0 && x + pw > tw {
                rows += 1;
                x = 0.0;
            }
            x += pw + 6.0;
        }
        text_h += 4.0 + rows as f32 * 20.0 + (rows - 1) as f32 * 6.0;
    }
    let pad = if compact { 7.0 } else { 12.0 };
    let h = (text_h + 2.0 * pad).max(if compact { 46.0 } else { 60.0 });
    let time = p.layout_no_wrap(ago(a.created_at.as_deref()), regular(12.5), FAINT);
    RowLayout { h, title, msg, pills, time, text_h }
}

/// Paints one activity row. Returns the id to replay when its button was pressed.
#[allow(clippy::too_many_arguments)]
fn paint_item(ui: &mut egui::Ui, icons: &Icons, a: &Activity, l: RowLayout, r: Rect, hovered: bool, fresh: f32, compact: bool, busy: bool) -> Option<String> {
    let p = ui.painter().clone();
    let alpha = fresh;
    let dy = (1.0 - fresh) * 5.0;
    let g = group_of(a.kind);
    let ts = if compact { 26.0 } else { 32.0 };
    let tr = Rect::from_min_size(pos2(r.left() + 16.0, r.center().y - ts / 2.0 + dy), vec2(ts, ts));
    tile(&p, icons, tr, g.icon, TILE.gamma_multiply(alpha), TILE_INK.gamma_multiply(alpha));
    let tx = tr.right() + 14.0;
    let mut ty = r.center().y - l.text_h / 2.0 + dy;
    let th = l.title.size().y.max(20.0);
    p.galley_with_override_text_color(pos2(tx, ty + (20.0 - l.title.rows.first().map_or(20.0, |r| r.height())).max(0.0) / 2.0), l.title.clone(), TEXT_2.gamma_multiply(alpha));
    // Keep the bold names bright: paint again with their own colors when fully in.
    if alpha >= 1.0 {
        p.galley(pos2(tx, ty + (20.0 - l.title.rows.first().map_or(20.0, |r| r.height())).max(0.0) / 2.0), l.title.clone(), TEXT);
    }
    ty += th;
    if let Some(m) = &l.msg {
        ty += 2.0;
        p.galley(pos2(tx, ty), m.clone(), TEXT);
        ty += m.size().y;
    }
    if !l.pills.is_empty() {
        ty += 4.0;
        let tw = r.right() - 16.0 - 102.0 - 14.0 - tx;
        let mut px = tx;
        for (g, test) in &l.pills {
            let pw = g.size().x + 14.0;
            if px > tx && px + pw > tx + tw {
                px = tx;
                ty += 26.0;
            }
            let pr = Rect::from_min_size(pos2(px, ty), vec2(pw, 20.0));
            p.rect_filled(pr, R_CONTROL, if *test { TAG_BG } else { BTN_IN });
            p.galley(pos2(px + 7.0, pr.center().y - g.size().y / 2.0), g.clone(), TEXT);
            px += pw + 6.0;
        }
    }
    // Side: time, then the replay button on hover.
    let side_r = r.right() - 16.0;
    let replay = Rect::from_min_size(pos2(side_r - 30.0, r.center().y - 15.0), vec2(30.0, 30.0));
    let time_right = replay.left() - 8.0;
    p.galley(pos2(time_right - l.time.size().x, r.center().y - l.time.size().y / 2.0), l.time.clone(), TEXT);
    if let Some(t) = a.created_at.as_deref().and_then(format::parse) {
        let tr = Rect::from_min_size(pos2(time_right - 64.0, r.center().y - 10.0), vec2(64.0, 20.0));
        ui.interact(tr, Id::unique(("time", &a.id)), Sense::hover()).on_hover_text(t.with_timezone(&chrono::Local).format("%c").to_string());
    }
    // Stream changes, polls and the like have no alert to replay.
    if a.kind != pine_core::ActivityKind::Other && hovered {
        let resp = ui.interact(replay, Id::unique(("replay", &a.id)), if busy { Sense::hover() } else { Sense::click() }).on_hover_text("Play this alert again");
        p.rect_filled(replay, R_CONTROL, (if resp.hovered() { BTN_IN_HOVER } else { BTN_IN }).gamma_multiply(if busy { 0.45 } else { 1.0 }));
        icons.paint(&p, "replay", replay.center(), 14.0, TEXT.gamma_multiply(if busy { 0.45 } else { 1.0 }));
        w::hand(ui, &resp);
        if resp.clicked() {
            return Some(a.id.clone());
        }
    }
    let _ = (Stroke::NONE, Pos2::ZERO);
    None
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        self.timers(&ctx);
        let full = ctx.content_rect();
        let fw = full.width();
        ui.painter().rect_filled(full, 0.0, FRAME);
        let bar = Rect::from_min_size(full.min, vec2(fw, TITLEBAR));
        self.titlebar(ui, bar, fw <= 480.0);
        let body = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
        let signed_out = self.restored && self.auth.sylva.is_none();
        if signed_out {
            let panel = Rect::from_min_max(pos2(body.left() + 8.0, body.top()), pos2(body.right() - 8.0, body.bottom() - 8.0));
            self.welcome(ui, panel);
        } else {
            let side_w = if fw <= 480.0 { 56.0 } else if fw <= 860.0 { 64.0 } else { 220.0 };
            let side = Rect::from_min_size(body.min, vec2(side_w, body.height()));
            let panel = Rect::from_min_max(pos2(side.right(), body.top()), body.max);
            ui.painter().rect_filled(panel, CornerRadius { nw: R_PANEL, ..Default::default() }, BG);
            self.sidebar(ui, side, fw <= 860.0);
            self.main_panel(ui, panel, fw);
        }
        self.settings_modal(&ctx);
        self.twitch_dialog(&ctx);
        self.update_dialog(&ctx);
        self.toasts.show(&ctx, full.bottom() - 20.0);
        chrome::edges(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(K_PAGE, self.page.key().into());
        storage.set_string(K_PREFS, serde_json::json!({ "adbar": self.prefs.adbar, "actions": self.prefs.actions, "compact": self.prefs.compact }).to_string());
        storage.set_string(K_SHOW, self.show.clone());
        storage.set_string(K_CLIENT, self.twitch_client_id.clone());
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = FRAME;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_art_asks_for_the_picker_size() {
        let c = Category { id: "509658".into(), name: "Just Chatting".into(), box_art_url: "https://static-cdn.jtvnw.net/ttv-boxart/509658-52x72.jpg".into() };
        assert_eq!(box_art_url(&c).unwrap(), "https://static-cdn.jtvnw.net/ttv-boxart/509658-72x96.jpg");
        let c = Category { id: "1".into(), name: "X".into(), box_art_url: String::new() };
        assert_eq!(box_art_url(&c).unwrap(), "https://static-cdn.jtvnw.net/ttv-boxart/1-72x96.jpg");
    }
}
