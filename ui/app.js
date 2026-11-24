"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

// ---------- small helpers ----------

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined && text !== null) n.textContent = String(text);
  return n;
}

function store(key, value) {
  try {
    if (value === undefined) return JSON.parse(localStorage.getItem(key));
    localStorage.setItem(key, JSON.stringify(value));
  } catch (_) {
    return null;
  }
  return null;
}

function toast(text, bad = false) {
  const t = el("div", "toast" + (bad ? " bad" : ""), text);
  $("toasts").appendChild(t);
  setTimeout(() => t.remove(), bad ? 6000 : 2800);
}

const fmt = new Intl.NumberFormat();
function num(n) {
  return n === null || n === undefined ? "-" : fmt.format(n);
}

function ago(iso) {
  if (!iso) return "";
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  if (s < 60) return "now";
  if (s < 3600) return Math.floor(s / 60) + "m";
  if (s < 86400) return Math.floor(s / 3600) + "h";
  return Math.floor(s / 86400) + "d";
}

function tierLabel(tier) {
  return { "1000": "Tier 1", "2000": "Tier 2", "3000": "Tier 3", prime: "Prime" }[tier] || null;
}

function money(amount, currency) {
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency: currency || "USD" }).format(amount);
  } catch (_) {
    return String(amount) + (currency ? " " + currency : "");
  }
}

function safeImg(url) {
  return typeof url === "string" && url.startsWith("https://") ? url : null;
}

// ---------- auth ----------

let auth = { streamelements: null, twitch: null, defaultTwitchClientId: null };

function applyAuth(a) {
  auth = a;
  $("signin").hidden = !!a.streamelements;
  $("menu-se-name").textContent = a.streamelements
    ? a.streamelements.displayName || a.streamelements.username || a.streamelements.id
    : "-";
  $("menu-tw-name").textContent = a.twitch ? a.twitch.login : "Not connected";
  $("tw-signout").hidden = !a.twitch;
  $("aud-connect").hidden = !!a.twitch;
  $("aud-body").hidden = !a.twitch;
  if (!a.twitch) {
    const saved = store("twitchClientId");
    $("tw-client-id").value = saved || a.defaultTwitchClientId || "";
  }
  if (!a.streamelements) setRealtime({ state: "stopped" });
}

$("signin-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const btn = $("signin-submit");
  const err = $("signin-error");
  err.hidden = true;
  btn.disabled = true;
  btn.textContent = "Checking token...";
  try {
    const a = await invoke("se_sign_in", { token: $("se-token").value });
    $("se-token").value = "";
    applyAuth(a);
    await loadFeed();
    refreshStats();
    loadAudience(true);
    toast("Signed in as " + (a.streamelements.displayName || a.streamelements.username));
  } catch (x) {
    err.textContent = String(x);
    err.hidden = false;
  } finally {
    btn.disabled = false;
    btn.textContent = "Sign in";
  }
});

$("account-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  $("account-menu").hidden = !$("account-menu").hidden;
});
document.addEventListener("click", (e) => {
  if (!$("account-menu").contains(e.target)) $("account-menu").hidden = true;
  if (!$("filter-menu").contains(e.target) && e.target !== $("filter-btn")) $("filter-menu").hidden = true;
});

$("se-signout").addEventListener("click", async () => {
  try {
    applyAuth(await invoke("se_sign_out"));
    feed = [];
    renderFeed();
    resetStats();
  } catch (x) {
    toast(String(x), true);
  }
});

$("tw-signout").addEventListener("click", async () => {
  try {
    applyAuth(await invoke("twitch_sign_out"));
    refreshStats();
  } catch (x) {
    toast(String(x), true);
  }
});

document.addEventListener("click", (e) => {
  const a = e.target.closest("a[data-ext]");
  if (!a) return;
  e.preventDefault();
  invoke("open_external", { url: a.dataset.ext }).catch((x) => toast(String(x), true));
});

// ---------- realtime status ----------

function setRealtime(s) {
  const p = $("rt-pill");
  p.className = "pill";
  switch (s.state) {
    case "connecting":
      p.textContent = "Connecting";
      p.classList.add("warn");
      break;
    case "connected":
      p.textContent = "Subscribing";
      p.classList.add("warn");
      break;
    case "live":
      p.textContent = "Realtime on";
      p.classList.add("ok");
      break;
    case "reconnecting":
      p.textContent = "Reconnecting in " + s.inSecs + "s";
      p.title = s.reason || "";
      p.classList.add("warn");
      break;
    case "unauthorized":
      p.textContent = "Realtime off";
      p.title = s.message || "";
      p.classList.add("bad");
      toast("Live events stopped: " + (s.message || "token rejected") + ". Sign in again.", true);
      break;
    default:
      p.textContent = "Not connected";
  }
}

// ---------- activity feed ----------

const KIND_GROUPS = [
  { id: "follow", label: "Follows", kinds: ["follow"] },
  { id: "sub", label: "Subscriptions", kinds: ["subscriber"] },
  { id: "gift", label: "Gifted subs", kinds: ["giftedSub", "communityGift"] },
  { id: "cheer", label: "Bits", kinds: ["cheer"] },
  { id: "tip", label: "Tips", kinds: ["tip"] },
  { id: "redemption", label: "Redemptions", kinds: ["redemption"] },
  { id: "raid", label: "Raids and hosts", kinds: ["raid", "host"] },
  { id: "other", label: "Other", kinds: ["other"] },
];
const MAX_ITEMS = 500;

let feed = [];
let hidden = new Set(store("hiddenGroups") || []);

function groupOf(kind) {
  const g = KIND_GROUPS.find((g) => g.kinds.includes(kind));
  return g ? g.id : "other";
}

function buildFilterMenu() {
  const m = $("filter-menu");
  m.replaceChildren();
  for (const g of KIND_GROUPS) {
    const label = el("label");
    const cb = el("input");
    cb.type = "checkbox";
    cb.checked = !hidden.has(g.id);
    cb.addEventListener("change", () => {
      if (cb.checked) hidden.delete(g.id);
      else hidden.add(g.id);
      store("hiddenGroups", [...hidden]);
      renderFeed();
    });
    label.append(cb, document.createTextNode(g.label));
    m.appendChild(label);
  }
  const row = el("div", "menu-actions");
  const all = el("button", null, "Show all");
  all.addEventListener("click", () => { hidden.clear(); store("hiddenGroups", []); buildFilterMenu(); renderFeed(); });
  row.appendChild(all);
  m.appendChild(row);
}

$("filter-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  $("filter-menu").hidden = !$("filter-menu").hidden;
});

function describe(a) {
  // Returns { badges: [{text, cls}], from, to, message }
  const name = a.displayName || a.username || "Someone";
  const tier = tierLabel(a.tier);
  const out = { badges: [], from: name, to: null, message: a.message };
  const add = (text, cls = "") => text && out.badges.push({ text, cls });
  switch (a.kind) {
    case "follow":
      add("Follow", "kind");
      break;
    case "subscriber":
      if (a.amount && a.amount > 1) add(a.amount + " months", "amount");
      add(tier || "Sub", "kind");
      break;
    case "giftedSub":
      add("Gift", "kind");
      add(tier, "amount");
      out.from = a.sender || "Anonymous";
      out.to = name;
      break;
    case "communityGift":
      if (a.amount) add(a.amount + "x", "amount");
      add("Gift subs", "kind");
      break;
    case "cheer":
      if (a.amount != null) add(num(a.amount) + (a.amount === 1 ? " bit" : " bits"), "amount");
      add("Bits", "kind");
      break;
    case "tip":
      if (a.amount != null) add(money(a.amount, a.currency), "amount");
      add("Tip", "kind");
      break;
    case "redemption":
      add("Reward", "kind");
      add(a.redemption, "amount");
      break;
    case "raid":
      if (a.amount != null) add(num(a.amount) + (a.amount === 1 ? " viewer" : " viewers"), "amount");
      add("Raid", "kind");
      break;
    case "host":
      if (a.amount != null) add(num(a.amount) + (a.amount === 1 ? " viewer" : " viewers"), "amount");
      add("Host", "kind");
      break;
    default:
      add(a.rawType, "kind");
  }
  return out;
}

function renderItem(a, fresh) {
  const d = describe(a);
  const li = el("li", "item k-" + a.kind + (fresh ? " fresh" : ""));
  li.dataset.id = a.id;

  const badges = el("div", "badges");
  for (const b of d.badges) badges.appendChild(el("span", "badge " + b.cls, b.text));

  const who = el("div", "who");
  const src = safeImg(a.avatar);
  if (src) {
    const img = el("img");
    img.src = src;
    img.alt = "";
    img.loading = "lazy";
    img.referrerPolicy = "no-referrer";
    img.onerror = () => img.remove();
    who.appendChild(img);
  }
  const names = el("span", "names", d.from);
  if (d.to) {
    names.appendChild(el("span", "arrow", " to "));
    names.appendChild(document.createTextNode(d.to));
  }
  who.appendChild(names);

  const time = el("time", "time", ago(a.createdAt));
  if (a.createdAt) {
    time.dateTime = a.createdAt;
    time.title = new Date(a.createdAt).toLocaleString();
  }

  li.append(badges, who, time);
  if (d.message) li.appendChild(el("div", "msg", d.message));

  const replay = el("button", "replay", "Replay");
  replay.title = "Play this alert again";
  replay.addEventListener("click", async () => {
    replay.disabled = true;
    try {
      await invoke("replay_activity", { id: a.id });
      toast("Alert replayed");
    } catch (x) {
      toast(String(x), true);
    } finally {
      replay.disabled = false;
    }
  });
  li.appendChild(replay);
  return li;
}

function visible(a) {
  return !hidden.has(groupOf(a.kind));
}

function renderFeed() {
  const list = $("feed");
  const items = feed.filter(visible);
  list.replaceChildren(...items.map((a) => renderItem(a, false)));
  $("feed-empty").hidden = items.length > 0;
  $("filter-count").textContent = hidden.size ? "(" + (KIND_GROUPS.length - hidden.size) + "/" + KIND_GROUPS.length + ")" : "";
}

function addLive(a) {
  if (feed.some((x) => x.id === a.id)) return;
  feed.unshift(a);
  if (feed.length > MAX_ITEMS) feed.length = MAX_ITEMS;
  if (visible(a)) {
    $("feed").prepend(renderItem(a, true));
    const extra = $("feed").children[MAX_ITEMS];
    if (extra) extra.remove();
    $("feed-empty").hidden = true;
  }
}

async function loadFeed() {
  if (!auth.streamelements) return;
  try {
    const history = await invoke("get_feed", { limit: 100 });
    const known = new Set(history.map((a) => a.id));
    // Keep live items that arrived while history was loading.
    feed = feed.filter((a) => !known.has(a.id)).concat(history);
    renderFeed();
  } catch (x) {
    toast("Could not load activity: " + x, true);
  }
}

$("feed-refresh").addEventListener("click", loadFeed);

setInterval(() => {
  for (const t of document.querySelectorAll("#feed time[datetime]")) t.textContent = ago(t.dateTime);
}, 30000);

// ---------- stats ----------

function resetStats() {
  for (const id of ["stat-viewers", "stat-chatters", "stat-followers", "stat-subs"]) $(id).textContent = "-";
  $("live-pill").hidden = true;
}

let statsTimer = null;
async function refreshStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(refreshStats, 30000);
  if (!auth.streamelements && !auth.twitch) return;
  try {
    const s = await invoke("get_stats");
    $("stat-followers").textContent = num(s.followers);
    $("stat-subs").textContent = num(s.subscribers);
    $("stat-viewers").textContent = num(s.viewers);
    $("stat-chatters").textContent = num(s.chatters);
    const live = $("live-pill");
    live.hidden = s.live === null || s.live === undefined;
    live.textContent = s.live ? "LIVE" : "OFFLINE";
    live.className = "pill" + (s.live ? " live" : "");
    const tip = s.problems.join("\n");
    for (const id of ["stat-viewers", "stat-chatters", "stat-followers", "stat-subs"]) {
      $(id).parentElement.dataset.problem = tip;
    }
    if (tip) console.warn(tip);
  } catch (x) {
    console.warn(x);
  }
}

let statsSoon = null;
function refreshStatsSoon() {
  clearTimeout(statsSoon);
  statsSoon = setTimeout(refreshStats, 3000);
}

// ---------- quick actions ----------

async function runAction(btn, fn, okText) {
  btn.disabled = true;
  try {
    await fn();
    if (okText) toast(okText);
    return true;
  } catch (x) {
    toast(String(x), true);
    return false;
  } finally {
    btn.disabled = false;
  }
}

function toggleButton(btn, onAction, offAction, onLabel, offLabel, onToast, offToast) {
  btn.addEventListener("click", async () => {
    const on = btn.dataset.on === "true";
    const ok = await runAction(btn, () => invoke("alert_action", { action: on ? offAction : onAction }), on ? offToast : onToast);
    if (ok) {
      btn.dataset.on = String(!on);
      btn.querySelector("span").textContent = on ? offLabel : onLabel;
    }
  });
}

toggleButton($("act-mute"), "mute", "unmute", "Unmute alerts", "Mute alerts", "Alerts muted", "Alerts unmuted");
toggleButton($("act-pause"), "pause", "unpause", "Resume alerts", "Pause alerts", "Alert queue paused", "Alert queue resumed");
$("act-skip").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("alert_action", { action: "skip" }), "Skipped the current alert"));
$("act-reload").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("reload_overlays"), "Overlays reloaded"));

// Timer: counts up from zero, or down from the minutes entered.
const timer = { running: false, startedAt: 0, banked: 0, handle: null };
function timerElapsed() {
  return timer.banked + (timer.running ? Date.now() - timer.startedAt : 0);
}
function timerTick() {
  const mins = Math.max(0, parseInt($("timer-minutes").value, 10) || 0);
  const elapsed = Math.floor(timerElapsed() / 1000);
  let secs = elapsed;
  if (mins > 0) {
    secs = mins * 60 - elapsed;
    if (secs <= 0) {
      secs = 0;
      timerStop();
      $("timer").classList.add("done");
    }
  }
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  $("timer-display").textContent = String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0");
}
function timerStop() {
  if (timer.running) timer.banked += Date.now() - timer.startedAt;
  timer.running = false;
  clearInterval(timer.handle);
  $("timer-start").textContent = "Start";
}
$("timer-start").addEventListener("click", () => {
  if (timer.running) {
    timerStop();
  } else {
    $("timer").classList.remove("done");
    timer.running = true;
    timer.startedAt = Date.now();
    timer.handle = setInterval(timerTick, 250);
    $("timer-start").textContent = "Pause";
  }
  timerTick();
});
$("timer-reset").addEventListener("click", () => {
  timerStop();
  timer.banked = 0;
  $("timer").classList.remove("done");
  timerTick();
});
$("timer-minutes").addEventListener("input", () => { if (!timer.running) timerTick(); });

// ---------- audience (Twitch) ----------

let audKind = "subscribers";
let audItems = [];
let audCursor = null;
let audTotal = null;
let audLoading = false;
let audRequest = 0;

for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll(".tab")) t.setAttribute("aria-selected", String(t === tab));
    audKind = tab.dataset.kind;
    loadAudience(true);
  });
}

function renderAudience() {
  const q = $("aud-search").value.trim().toLowerCase();
  const list = $("aud-list");
  const rows = audItems.filter((u) => !q || u.user_name.toLowerCase().includes(q) || u.user_login.includes(q));
  list.replaceChildren(
    ...rows.map((u) => {
      const li = el("li");
      li.appendChild(el("span", null, u.user_name));
      let meta = "";
      if (audKind === "subscribers") {
        meta = tierLabel(u.tier) || u.tier;
        if (u.is_gift) meta += u.gifter_name ? " gift from " + u.gifter_name : " gift";
      } else if (audKind === "followers" && u.followed_at) {
        meta = new Date(u.followed_at).toLocaleDateString();
      }
      if (meta) li.appendChild(el("span", "sub-meta", meta));
      return li;
    })
  );
  const label = { subscribers: "subscribers", followers: "followers", chatters: "in chat" }[audKind];
  $("aud-total").textContent = audTotal !== null ? num(audTotal) + " " + label : "";
  $("aud-more").hidden = !audCursor;
  const empty = $("aud-empty");
  empty.hidden = rows.length > 0 || audLoading;
  empty.textContent = q ? "No loaded names match." : "Nobody here yet.";
}

async function loadAudience(reset) {
  if (!auth.twitch) return;
  // "Load more" waits for the current page; a reset always wins over older requests.
  if (audLoading && !reset) return;
  if (reset) {
    audItems = [];
    audCursor = null;
    audTotal = null;
    renderAudience();
  }
  const req = ++audRequest;
  audLoading = true;
  try {
    const page = await invoke("get_audience", { kind: audKind, cursor: reset ? null : audCursor });
    if (req !== audRequest) return;
    audItems = audItems.concat(page.items);
    audCursor = page.cursor;
    audTotal = page.total ?? audTotal;
  } catch (x) {
    if (req === audRequest) toast(String(x), true);
  } finally {
    if (req === audRequest) {
      audLoading = false;
      renderAudience();
    }
  }
}

$("aud-search").addEventListener("input", renderAudience);
$("aud-more").addEventListener("click", () => loadAudience(false));
$("aud-refresh").addEventListener("click", () => loadAudience(true));

let deviceUri = null;
$("tw-connect").addEventListener("click", async () => {
  const clientId = $("tw-client-id").value.trim();
  const btn = $("tw-connect");
  btn.disabled = true;
  try {
    const code = await invoke("twitch_sign_in", { clientId });
    store("twitchClientId", clientId);
    deviceUri = code.verification_uri;
    $("tw-user-code").textContent = code.user_code;
    $("tw-code").hidden = false;
  } catch (x) {
    toast(String(x), true);
    btn.disabled = false;
  }
});
$("tw-open-again").addEventListener("click", (e) => {
  e.preventDefault();
  if (deviceUri) invoke("open_external", { url: deviceUri }).catch((x) => toast(String(x), true));
});
$("tw-cancel").addEventListener("click", async (e) => {
  e.preventDefault();
  await invoke("twitch_cancel_sign_in");
  $("tw-code").hidden = true;
  $("tw-connect").disabled = false;
});

// ---------- startup ----------

async function start() {
  buildFilterMenu();
  timerTick();

  await listen("activity", (e) => {
    addLive(e.payload);
    refreshStatsSoon();
  });
  await listen("realtime-status", (e) => setRealtime(e.payload));
  await listen("twitch-auth", async (e) => {
    $("tw-code").hidden = true;
    $("tw-connect").disabled = false;
    if (e.payload.ok) {
      toast("Twitch connected as " + e.payload.login);
      applyAuth(await invoke("restore_session"));
      refreshStats();
      loadAudience(true);
    } else {
      toast("Twitch sign-in failed: " + e.payload.error, true);
    }
  });

  try {
    const a = await invoke("restore_session");
    applyAuth(a);
    if (a.notice) toast(a.notice, true);
  } catch (x) {
    toast(String(x), true);
    applyAuth(auth);
  }
  await loadFeed();
  refreshStats();
  loadAudience(true);
}

start();
