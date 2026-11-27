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

function icon(id) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(ns, "use");
  use.setAttribute("href", "#" + id);
  svg.appendChild(use);
  return svg;
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

function toast(text, bad = false, ms = bad ? 6500 : 3000) {
  const t = el("div", "toast" + (bad ? " bad" : ""), text);
  $("toasts").appendChild(t);
  setTimeout(() => {
    t.classList.add("out");
    setTimeout(() => t.remove(), 160);
  }, ms);
}

/** A grey placeholder the size of what is coming, so nothing moves when it arrives. */
function skel(w, h) {
  const s = el("span", "skel");
  s.style.width = typeof w === "number" ? w + "px" : w;
  s.style.height = h + "px";
  return s;
}

function skelRows(n) {
  const widths = ["58%", "44%", "66%", "38%", "52%", "61%"];
  return Array.from({ length: n }, (_, i) => {
    const row = el("li", "skel-row");
    row.setAttribute("aria-hidden", "true");
    const lines = el("div", "skel-lines");
    lines.append(skel(widths[i % widths.length], 13), skel("28%", 10));
    row.append(skel(32, 32), lines);
    return row;
  });
}

const fmt = new Intl.NumberFormat();
function num(n) {
  return n === null || n === undefined ? "-" : fmt.format(n);
}

function ago(iso) {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, (Date.now() - t) / 1000);
  if (s < 60) return "now";
  if (s < 3600) return Math.floor(s / 60) + "m ago";
  if (s < 86400) return Math.floor(s / 3600) + "h ago";
  return Math.floor(s / 86400) + "d ago";
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

function avatar(name, url) {
  const a = el("span", "avatar");
  const letter = (name || "?").trim().charAt(0).toUpperCase() || "?";
  const src = safeImg(url);
  if (src) {
    const img = el("img");
    img.src = src;
    img.alt = "";
    img.referrerPolicy = "no-referrer";
    img.onerror = () => { img.remove(); a.textContent = letter; };
    a.appendChild(img);
  } else {
    a.textContent = letter;
  }
  return a;
}

// Anything that moves stops while the window is hidden or minimized.
function markHidden() {
  document.documentElement.toggleAttribute("data-hidden", document.hidden);
}
document.addEventListener("visibilitychange", markHidden);
markHidden();

// ---------- pages ----------

const PAGES = ["home", "activity", "stream"];
let page = PAGES.includes(store("page")) ? store("page") : "home";

function go(to) {
  if (!PAGES.includes(to)) return;
  const changed = to !== page;
  page = to;
  store("page", to);
  for (const p of PAGES) $("page-" + p).hidden = p !== to;
  for (const b of document.querySelectorAll(".side .si[data-page]")) {
    if (b.dataset.page === to) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  }
  if (changed) $("main").scrollTop = 0;
  if (to === "stream") fitTitle();
}

for (const b of document.querySelectorAll("[data-page], [data-go]")) {
  b.addEventListener("click", () => go(b.dataset.page || b.dataset.go));
}
go(page);

// ---------- settings ----------

let settingsFrom = null;

function openSettings(pane) {
  settingsFrom = document.activeElement;
  $("settings").hidden = false;
  showSettingsPane(pane || "accounts");
  $("settings-close").focus();
}

function closeSettings() {
  if ($("settings").hidden) return;
  $("settings").hidden = true;
  if (settingsFrom && settingsFrom.focus) settingsFrom.focus();
}

const PANE_TITLES = { accounts: "Accounts", display: "Display", about: "About" };
function showSettingsPane(pane) {
  for (const b of document.querySelectorAll("[data-set]")) {
    if (b.dataset.set === pane) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  }
  for (const p of document.querySelectorAll("[data-set-pane]")) p.hidden = p.dataset.setPane !== pane;
  $("set-heading").textContent = PANE_TITLES[pane];
}

$("settings-btn").addEventListener("click", () => openSettings());
$("settings-close").addEventListener("click", closeSettings);
for (const b of document.querySelectorAll("[data-set]")) b.addEventListener("click", () => showSettingsPane(b.dataset.set));
for (const back of document.querySelectorAll(".modal-back")) {
  back.addEventListener("mousedown", (e) => {
    if (e.target !== back) return;
    if (back.id === "settings") closeSettings();
    else if (back.id === "twitch-dialog") closeTwitchDialog();
  });
}

// Display preferences, saved on this computer.
const PREF_DEFAULTS = { adbar: true, actions: true, compact: false };
const prefs = { ...PREF_DEFAULTS, ...(store("prefs") || {}) };

function applyPrefs() {
  const b = document.body.classList;
  b.toggle("no-adbar", !prefs.adbar);
  b.toggle("no-actions", !prefs.actions);
  b.toggle("compact", !!prefs.compact);
  for (const s of document.querySelectorAll("#prefs [data-pref]")) s.setAttribute("aria-checked", String(!!prefs[s.dataset.pref]));
}

for (const s of document.querySelectorAll("#prefs [data-pref]")) {
  s.addEventListener("click", () => {
    prefs[s.dataset.pref] = !prefs[s.dataset.pref];
    store("prefs", prefs);
    applyPrefs();
  });
}
applyPrefs();

if (window.__TAURI__.app) {
  window.__TAURI__.app.getVersion().then((v) => { $("app-version").textContent = v; }).catch(() => {});
}

// ---------- window buttons (the system title bar is hidden) ----------

const appWindow = window.__TAURI__.window.getCurrentWindow();
$("win-min").addEventListener("click", () => appWindow.minimize().catch(console.warn));
$("win-max").addEventListener("click", () => appWindow.toggleMaximize().catch(console.warn));
$("win-close").addEventListener("click", () => appWindow.close().catch(console.warn));

// Drag the window from the title bar. Sylva Control starts the drag itself (instead of Tauri's
// built-in drag region) so that a window Windows has snapped keeps its current size when
// moved, rather than jumping back to the size it had before it was snapped or resized.
document.querySelector(".titlebar").addEventListener("mousedown", (e) => {
  const target = e.target.closest("[data-drag-region]");
  if (e.button !== 0 || !target || e.target.closest("button")) return;
  e.preventDefault();
  if (e.detail === 2) appWindow.toggleMaximize().catch(console.warn);
  else if (e.detail === 1) invoke("start_window_drag").catch(console.warn);
});
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (!$("twitch-dialog").hidden) closeTwitchDialog();
  else closeSettings();
});

// ---------- auth ----------

let auth = { sylva: null, twitch: null, defaultTwitchClientId: null };

function greeting() {
  const h = new Date().getHours();
  return h < 5 ? "Good evening" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
}

function applyAuth(a) {
  const before = auth.sylva ? auth.sylva.key : null;
  auth = a;
  const sy = a.sylva;
  $("signin").hidden = !!sy;
  document.body.classList.toggle("signed-out", !sy);
  const name = sy ? sy.name || sy.login || "Streamer" : "";
  $("me-name").textContent = name;
  $("me-sub").textContent = a.twitch ? "Twitch connected" : "Twitch not connected";
  const av = avatar(name || "?", sy && sy.avatar);
  av.id = "user-avatar";
  if (!sy) av.textContent = "";
  $("user-avatar").replaceWith(av);
  $("home-title").textContent = name ? greeting() + ", " + name : "Welcome back";
  $("menu-sy-name").textContent = sy ? "Signed in as " + name : "Not signed in";
  $("menu-tw-name").textContent = a.twitch ? "Connected as " + a.twitch.login : "Not connected";
  $("tw-signout").hidden = !a.twitch;
  $("tw-connect-menu").hidden = !!a.twitch;
  $("sy-signout").hidden = !sy;
  // Another channel (or signed out): start the feed over.
  if ((sy ? sy.key : null) !== before) {
    feed = [];
    feedLoaded = false;
    resetStats();
    renderFeed();
  }
  loadChannelInfo();
  refreshAds();
  refreshAlertState();
  loadHeldTips();
}

// ---------- held tips ----------
// Big, first-time or risky tips wait in Sylva until someone approves them.

async function loadHeldTips() {
  if (!auth.sylva) { $("sec-tips").hidden = true; return; }
  let tips = [];
  try {
    tips = await invoke("get_held_tips");
  } catch (_) {
    return;
  }
  $("sec-tips").hidden = tips.length === 0;
  $("held-tips").replaceChildren(...tips.map((t) => {
    const li = el("li", "gr");
    const tile = el("span", "tile");
    tile.appendChild(icon("i-tip"));
    const main = el("div", "gr-main");
    const top = el("div", "t1");
    top.append(el("span", null, t.name + " "), el("span", "held-amount", money(t.amount, t.currency)));
    main.append(top);
    if (t.message) main.append(el("p", "held-msg", t.message));
    const why = [t.holdReason, t.country ? "from " + t.country : null].filter(Boolean).join(", ");
    if (why) main.append(el("span", "t2", why.charAt(0).toUpperCase() + why.slice(1)));
    const show = el("button", "btn sm primary", "Show it");
    const refund = el("button", "btn sm", "Refund");
    show.type = refund.type = "button";
    const decide = async (btn, approve) => {
      show.disabled = refund.disabled = true;
      const ok = await runAction(btn, () => invoke("decide_tip", { id: t.id, approve }), approve ? "Tip sent to the stream" : "Tip refunded");
      if (ok) li.remove();
      else show.disabled = refund.disabled = false;
      $("sec-tips").hidden = $("held-tips").children.length === 0;
    };
    show.addEventListener("click", () => decide(show, true));
    refund.addEventListener("click", () => decide(refund, false));
    const side = el("div", "gr-ctl");
    side.append(refund, show);
    li.append(tile, main, side);
    return li;
  }));
}
// A slow check in case a change was missed while offline.
setInterval(() => { if (!document.hidden) loadHeldTips(); }, 60000);

// ---------- signing in with Sylva ----------
// Sylva is where alerts, overlays and the bot live. Sylva Control shows Sylva's
// activity, the same feed every Sylva page shows, and controls its alerts.

$("signin-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const btn = $("signin-submit");
  const err = $("signin-error");
  err.hidden = true;
  btn.disabled = true;
  btn.textContent = "Waiting for the browser...";
  try {
    const code = await invoke("sylva_sign_in");
    const hint = $("signin-code");
    hint.textContent = "Approve Sylva Control in your browser. Your code is " + code.user_code + ".";
    hint.hidden = false;
  } catch (x) {
    err.textContent = "Could not start sign-in: " + x;
    err.hidden = false;
    btn.disabled = false;
    btn.textContent = "Sign in with Sylva";
  }
});

$("sy-signout").addEventListener("click", async () => {
  closeSettings();
  try {
    applyAuth(await invoke("sylva_sign_out"));
  } catch (x) {
    toast(String(x), true);
  }
});

$("tw-signout").addEventListener("click", async () => {
  try {
    applyAuth(await invoke("twitch_sign_out"));
    refreshViewers();
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

// ---------- activity feed ----------

// The same groups, names and icons as Sylva's activity.
const GROUPS = [
  { id: "follow", label: "Follows", icon: "i-heart", kinds: ["follow"] },
  { id: "sub", label: "Subs", icon: "i-star", kinds: ["subscriber"] },
  { id: "gift", label: "Gifts", icon: "i-gift", kinds: ["giftedSub", "communityGift"] },
  { id: "cheer", label: "Bits", icon: "i-zap", kinds: ["cheer"] },
  { id: "raid", label: "Raids", icon: "i-raid", kinds: ["raid", "host"] },
  { id: "tip", label: "Tips", icon: "i-tip", kinds: ["tip"] },
  { id: "redemption", label: "Redemptions", icon: "i-gem", kinds: ["redemption"] },
  { id: "other", label: "Other", icon: "i-sparkles", kinds: ["other"] },
];
const KNOWN_KINDS = new Set(GROUPS.flatMap((g) => g.kinds));
const CHIPS = [{ id: "all", label: "All" }, ...GROUPS.filter((g) => g.id !== "other").map((g) => ({ id: g.id, label: g.label }))];
const MAX_ITEMS = 500;
const RECENT = 6;
const DAY = 86400000;

let feed = [];
let feedLoaded = false;
// Test events leave the feed 30 seconds after they came in, as on Sylva.
const TEST_TTL = 30000;
let show = CHIPS.some((c) => c.id === store("activityShow")) ? store("activityShow") : "all";
let query = "";

function groupOf(kind) {
  return GROUPS.find((g) => g.kinds.includes(kind)) || GROUPS[GROUPS.length - 1];
}

/**
 * One sentence that says what happened, with names in bold, plus small pills
 * for the details (tier, months).
 */
function describe(a) {
  const name = a.displayName || a.username || "Someone";
  const n = a.amount;
  const has = n !== null && n !== undefined;
  const plural = (one, many) => (n === 1 ? one : many);
  const tier = tierLabel(a.tier);
  const parts = [];
  const pills = [];
  let message = a.message;
  const b = (t) => parts.push(el("b", null, t));
  const t = (s) => parts.push(document.createTextNode(s));

  switch (a.kind) {
    case "follow":
      b(name); t(" followed");
      break;
    case "subscriber":
      b(name); t(has && n > 1 ? " resubscribed" : " subscribed");
      if (tier) pills.push(tier);
      if (has && n > 1) pills.push(num(n) + " months");
      break;
    case "giftedSub":
      if (a.sender) { b(a.sender); t(" gifted a sub to "); b(name); }
      else { b(name); t(" got a gifted sub"); }
      if (tier) pills.push(tier);
      // Twitch fills in an automatic "X gifted a sub to Y" message; the sentence already says it.
      if (/gift/i.test(message || "")) message = null;
      break;
    case "communityGift":
      b(name); t(has ? " gifted " + num(n) + " " + plural("sub", "subs") : " gifted subs");
      if (tier) pills.push(tier);
      if (/gift/i.test(message || "")) message = null;
      break;
    case "cheer":
      b(name); t(has ? " cheered " + num(n) + " " + plural("bit", "bits") : " cheered");
      break;
    case "tip":
      b(name); t(has ? " tipped " + money(n, a.currency) : " tipped");
      break;
    case "redemption":
      b(name); t(" redeemed "); b(a.redemption || "a reward");
      break;
    case "raid":
      b(name); t(has ? " raided with " + num(n) + " " + plural("viewer", "viewers") : " raided");
      break;
    case "host":
      b(name); t(has ? " hosted with " + num(n) + " " + plural("viewer", "viewers") : " hosted");
      break;
    default: {
      const w = otherWords(a);
      parts.push(...w.parts);
      pills.push(...w.pills);
      if (w.message !== undefined) message = w.message;
    }
  }
  return { parts, pills, message };
}

// Words for Twitch events that are not follows, subs and the like, from the
// payload Sylva passes along. The same sentences Sylva's own lists use.
function otherWords(a) {
  const raw = a.raw || {};
  const str = (k) => (typeof raw[k] === "string" && raw[k] ? raw[k] : null);
  const int = (k) => (typeof raw[k] === "number" && isFinite(raw[k]) ? raw[k] : null);
  const parts = [];
  const pills = [];
  let message;
  const b = (t) => parts.push(el("b", null, t));
  const t = (s) => parts.push(document.createTextNode(s));
  const type = a.rawType || "";
  const ends = (x) => type.endsWith(x);
  const winner = (list, id) => {
    if (!Array.isArray(list)) return null;
    if (id) { const o = list.find((x) => x && x.id === id); return o && o.title ? o.title : null; }
    const score = (o) => Number(o.votes ?? o.channel_points ?? 0) || 0;
    const best = list.reduce((m, o) => (o && (!m || score(o) > score(m)) ? o : m), null);
    return best && score(best) > 0 ? best.title || null : null;
  };
  const secs = (n) => {
    if (n < 60) return n + " seconds";
    const m = Math.floor(n / 60), r = n % 60;
    return r ? m + ":" + String(r).padStart(2, "0") : m + (m === 1 ? " minute" : " minutes");
  };
  switch (type) {
    case "stream.online": b("You"); t(" went live"); break;
    case "stream.offline": b("Stream"); t(" ended"); break;
    case "channel.update":
      b("Stream info"); t(" changed");
      message = [str("title"), str("category_name")].filter(Boolean).join(" · ") || null;
      break;
    case "channel.raid":
      b("You"); t(" raided "); b(str("to_broadcaster_user_name") || "a channel");
      if (a.amount) pills.push(num(a.amount) + (a.amount === 1 ? " viewer" : " viewers"));
      break;
    case "channel.ad_break.begin":
      b("Ad break"); t(" started");
      if (int("duration_seconds")) pills.push(secs(int("duration_seconds")));
      if (raw.is_automatic === true) pills.push("Automatic");
      else if (str("requester_user_name")) pills.push("Started by " + str("requester_user_name"));
      break;
    case "channel.hype_train.begin":
    case "channel.hype_train.progress":
    case "channel.hype_train.end":
      b("Hype train"); t(ends("begin") ? " started" : ends("end") ? " ended" : " is going");
      if (int("level")) pills.push("Level " + num(int("level")));
      if (int("total")) pills.push(num(int("total")) + " points");
      break;
    case "channel.prediction.begin":
    case "channel.prediction.lock":
    case "channel.prediction.end":
      b("Prediction"); t(ends("begin") ? " started" : ends("lock") ? " locked" : " ended");
      message = str("title");
      if (ends("end")) {
        const won = winner(raw.outcomes, str("winning_outcome_id"));
        pills.push(won ? "Won: " + won : "Canceled");
      }
      break;
    case "channel.poll.begin":
    case "channel.poll.end": {
      b("Poll"); t(ends("begin") ? " started" : " ended");
      message = str("title");
      const top = ends("end") ? winner(raw.choices) : null;
      if (top) pills.push("Top: " + top);
      break;
    }
    case "channel.charity_campaign.donate": {
      b(str("user_name") || a.displayName || "Someone");
      const charity = str("charity_name");
      if (charity) { t(" donated to "); b(charity); } else t(" donated to charity");
      const amt = raw.amount;
      if (amt && typeof amt.value === "number") {
        const v = amt.value / Math.pow(10, amt.decimal_places || 0);
        pills.push(money(v, amt.currency || "USD"));
      }
      break;
    }
    default: {
      const words = type.replace(/^channel\./, "").replace(/[._]+/g, " ").trim();
      const human = words ? words[0].toUpperCase() + words.slice(1) : "Event";
      if (a.displayName || a.username) { b(a.displayName || a.username); t(" " + human.toLowerCase()); }
      else b(human);
    }
  }
  return { parts, pills, message };
}

function renderItem(a, fresh) {
  const g = groupOf(a.kind);
  const d = describe(a);
  const kind = KNOWN_KINDS.has(a.kind) ? a.kind : "other";
  const li = el("li", "item" + (fresh ? " fresh" : ""));
  li.dataset.id = a.id;

  const tile = el("span", "tile");
  tile.appendChild(icon(g.icon));

  const main = el("div", "item-main");
  const title = el("div", "item-title");
  title.append(...d.parts);
  main.appendChild(title);
  if (d.message) main.appendChild(el("p", "item-msg", d.message));
  if (d.pills.length || a.test) {
    const pills = el("div", "pills");
    for (const p of d.pills) pills.appendChild(el("span", "pill", p));
    if (a.test) pills.appendChild(el("span", "pill test", "test"));
    main.appendChild(pills);
  }

  const side = el("div", "item-side");
  const time = el("time", null, ago(a.createdAt));
  if (a.createdAt) {
    time.dateTime = a.createdAt;
    time.title = new Date(a.createdAt).toLocaleString();
  }
  side.appendChild(time);

  // Stream changes, polls and the like have no alert to replay.
  if (kind === "other") {
    side.appendChild(el("span", "no-replay"));
  } else {
    const replay = el("button", "replay");
    replay.type = "button";
    replay.title = "Play this alert again";
    replay.setAttribute("aria-label", "Play this alert again");
    replay.appendChild(icon("i-replay"));
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
    side.appendChild(replay);
  }
  li.append(tile, main, side);
  return li;
}

function matches(a) {
  if (show !== "all" && groupOf(a.kind).id !== show) return false;
  if (!query) return true;
  const hay = [a.displayName, a.username, a.sender, a.message, a.redemption].filter(Boolean).join(" ").toLowerCase();
  return hay.includes(query);
}

function dayKey(iso) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "unknown" : d.getFullYear() + "-" + d.getMonth() + "-" + d.getDate();
}

function dayName(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "Earlier";
  const today = new Date();
  const d0 = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const t0 = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const days = Math.round((t0 - d0) / DAY);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric", ...(d.getFullYear() !== today.getFullYear() ? { year: "numeric" } : {}) });
}

function daySection(key, iso) {
  const sec = el("section", "sect act-day");
  sec.dataset.day = key;
  const head = el("div", "sect-head");
  const h = el("div");
  h.appendChild(el("h2", null, dayName(iso)));
  head.appendChild(h);
  const box = el("div", "grp");
  const ol = el("ol", "feed");
  box.appendChild(ol);
  sec.append(head, box);
  return sec;
}

/** The latest few on Home. */
function renderRecent() {
  const list = $("recent");
  if (!feedLoaded) {
    list.replaceChildren(...skelRows(4));
    $("recent-empty").hidden = true;
    return;
  }
  list.replaceChildren(...feed.slice(0, RECENT).map((a) => renderItem(a, false)));
  $("recent-empty").hidden = feed.length > 0;
}

/** Everything on Activity, under a heading per day. */
function renderActivity() {
  const days = $("act-days");
  if (!feedLoaded) {
    const sec = daySection("loading", null);
    sec.querySelector("h2").replaceChildren(skel(60, 12));
    sec.querySelector("ol").replaceChildren(...skelRows(6));
    days.replaceChildren(sec);
    $("act-empty").hidden = true;
    return;
  }
  const items = feed.filter(matches);
  const sections = [];
  let cur = null;
  for (const a of items) {
    const key = dayKey(a.createdAt);
    if (!cur || cur.dataset.day !== key) {
      cur = daySection(key, a.createdAt);
      sections.push(cur);
    }
    cur.querySelector("ol").appendChild(renderItem(a, false));
  }
  days.replaceChildren(...sections);
  $("act-empty").hidden = items.length > 0;
  $("act-empty-text").textContent = feed.length
    ? "Nothing here for this filter yet."
    : "No activity yet. Follows, subs, bits and rewards appear here the moment they happen.";
}

function renderChips() {
  $("act-chips").replaceChildren(...CHIPS.map((c) => {
    const b = el("button", "btn sm", c.label);
    b.type = "button";
    b.setAttribute("aria-pressed", String(show === c.id));
    b.addEventListener("click", () => {
      show = c.id;
      store("activityShow", show);
      renderChips();
      renderActivity();
    });
    return b;
  }));
}

function renderFeed() {
  renderRecent();
  renderActivity();
}

let searchTimer = null;
$("act-search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    query = $("act-search").value.trim().toLowerCase();
    renderActivity();
  }, 120);
});

function removeRows(id) {
  for (const li of document.querySelectorAll(".feed li[data-id]")) {
    if (li.dataset.id !== id) continue;
    const sec = li.closest(".act-day");
    li.remove();
    if (sec && !sec.querySelector("li")) sec.remove();
  }
}

/** Take a test event out of the feed once its 30 seconds are up. */
function expireTest(a, live) {
  if (a.expires) return;
  const left = live || !a.receivedAt ? TEST_TTL : Math.min(TEST_TTL, Math.max(0, a.receivedAt + TEST_TTL - Date.now()));
  a.expires = setTimeout(() => {
    feed = feed.filter((x) => x !== a);
    removeRows(a.id);
    renderRecent();
    $("act-empty").hidden = feed.some(matches);
  }, left);
}

function addLive(a) {
  if (feed.some((x) => x.id === a.id)) return;
  if (a.test) expireTest(a, true);
  feed.unshift(a);
  if (feed.length > MAX_ITEMS) feed.length = MAX_ITEMS;
  if (!feedLoaded) return;

  // Home: the new one slides in on top, the oldest of the few leaves.
  $("recent").prepend(renderItem(a, true));
  while ($("recent").children.length > RECENT) $("recent").lastElementChild.remove();
  $("recent-empty").hidden = true;

  // Activity: into today's list when it is already there, otherwise draw the page again.
  if (!matches(a)) return;
  const first = $("act-days").firstElementChild;
  if (first && first.dataset.day === dayKey(a.createdAt)) {
    first.querySelector("ol").prepend(renderItem(a, true));
    $("act-empty").hidden = true;
  } else {
    renderActivity();
  }
}

async function loadFeed() {
  if (!auth.sylva) return;
  const btn = $("feed-refresh");
  btn.classList.add("spin");
  btn.disabled = true;
  try {
    const history = await invoke("get_feed", { limit: 250 });
    const known = new Set(history.map((a) => a.id));
    // Keep live items that arrived while history was loading.
    feed = feed.filter((a) => !known.has(a.id)).concat(history).slice(0, MAX_ITEMS);
    for (const a of feed) if (a.test) expireTest(a, false);
  } catch (x) {
    toast("Could not load activity: " + x, true);
  } finally {
    feedLoaded = true;
    renderFeed();
    btn.classList.remove("spin");
    btn.disabled = false;
  }
}

$("feed-refresh").addEventListener("click", loadFeed);

setInterval(() => {
  if (document.hidden) return;
  for (const t of document.querySelectorAll(".feed time[datetime]")) t.textContent = ago(t.dateTime);
}, 30000);

// ---------- numbers ----------

function resetStats() {
  $("stat-viewers").replaceChildren(skel(72, 26));
  $("stat-followers").replaceChildren(skel(96, 26));
  $("stat-subs").replaceChildren(skel(72, 26));
  $("viewer-foot").className = "total-foot";
  $("viewer-foot").replaceChildren(skel(60, 11));
}

/** The line under the viewer count: live, offline, or why there is no number. */
function setViewerFoot(text, live = false, detail = "") {
  const f = $("viewer-foot");
  f.className = "total-foot" + (live ? " live" : "");
  f.textContent = text;
  f.title = detail;
}

let statsTimer = null;
async function refreshStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(refreshStats, 60000);
  if (!auth.sylva) return;
  try {
    const s = await invoke("get_stats");
    $("stat-followers").textContent = num(s.followers);
    $("stat-subs").textContent = num(s.subscribers);
  } catch (x) {
    console.warn(x);
    for (const id of ["stat-followers", "stat-subs"]) if ($(id).querySelector(".skel")) $(id).textContent = "-";
  }
}

let statsSoon = null;
function refreshStatsSoon() {
  clearTimeout(statsSoon);
  statsSoon = setTimeout(refreshStats, 3000);
}

// Viewers change all the time, so they are checked often. No Twitch sign-in is needed.
// With Twitch connected the count comes from the official API, which is cheap to ask every
// 5 seconds. Without it, the public lookup is asked every 10 seconds to stay polite.
// A hidden window checks once a minute.
const VIEWER_POLL_MS = { twitch: 5000, public: 10000, none: 30000 };
let viewerTimer = null;
let viewerSource = "public";
let viewerRun = 0;
async function refreshViewers() {
  clearTimeout(viewerTimer);
  const run = ++viewerRun;
  try {
    const v = await invoke("get_viewers");
    viewerSource = v.source;
    const n = $("stat-viewers");
    if (v.source === "none") {
      n.textContent = "-";
      setViewerFoot(auth.sylva ? "No Twitch channel on this account" : "Not available");
      return;
    }
    // Offline shows a dash, not 0, so it never looks like an empty stream.
    n.textContent = v.live ? num(v.viewers) : "-";
    n.title = "Checked at " + new Date().toLocaleTimeString();
    setViewerFoot(v.live ? "Live now" : "Offline", v.live);
  } catch (x) {
    $("stat-viewers").textContent = "-";
    setViewerFoot("Viewer count unavailable", false, String(x));
  } finally {
    // Only the latest check schedules the next one, so checks never pile up.
    const wait = document.hidden ? 60000 : VIEWER_POLL_MS[viewerSource] || 10000;
    if (run === viewerRun) viewerTimer = setTimeout(refreshViewers, wait);
  }
}

// Refresh right away when the window comes back into view.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) return;
  refreshViewers();
  if (auth.twitch) loadChannelInfo(true);
  if (ads) renderAd();
});

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

// Each switch-like action: its words and icon when off, then when on.
const TOGGLES = {
  "act-mute": {
    actions: ["mute", "unmute"],
    off: { t1: "Mute alerts", t2: "Alerts play with sound", icon: "i-vol" },
    on: { t1: "Unmute alerts", t2: "Alerts are muted", icon: "i-volx" },
    toasts: ["Alerts muted", "Alerts unmuted"],
  },
  "act-pause": {
    actions: ["pause", "unpause"],
    off: { t1: "Pause alerts", t2: "Alerts play as they come", icon: "i-pause" },
    on: { t1: "Resume alerts", t2: "Alerts wait in line", icon: "i-play" },
    toasts: ["Alerts paused", "Alerts resumed"],
  },
};

/** Show a toggle as on or off. */
function setToggle(id, on) {
  const btn = $(id);
  const w = on ? TOGGLES[id].on : TOGGLES[id].off;
  btn.setAttribute("aria-pressed", String(on));
  btn.querySelector(".t1").textContent = w.t1;
  btn.querySelector(".t2").textContent = w.t2;
  btn.querySelector("use").setAttribute("href", "#" + w.icon);
}

for (const id of Object.keys(TOGGLES)) {
  const t = TOGGLES[id];
  $(id).addEventListener("click", async () => {
    const on = $(id).getAttribute("aria-pressed") === "true";
    const ok = await runAction($(id), () => invoke("alert_action", { action: on ? t.actions[1] : t.actions[0] }), on ? t.toasts[1] : t.toasts[0]);
    if (ok) setToggle(id, !on);
  });
}

/** Start the mute and pause buttons the way Sylva's queue is now. */
async function refreshAlertState() {
  if (!auth.sylva) return;
  try {
    const s = await invoke("get_alert_state");
    setToggle("act-mute", !!s.muted);
    setToggle("act-pause", !!s.paused);
  } catch (x) {
    console.warn(x);
  }
}

$("act-skip").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("alert_action", { action: "skip" }), "Skipped the current alert"));
$("act-reload").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("reload_overlays"), "Overlays reloaded"));

// ---------- stream title and category ----------

// What Twitch has now, so Save only sends what changed.
let channelInfo = null;
// The category picked from the list: { id, name, art }.
let pickedCategory = null;
let savingInfo = false;

/** Box art for a category, sized for the picker. Twitch serves any size from the same link. */
function boxArt(c) {
  if (!c || !c.id) return null;
  const url = c.box_art_url || c.art || "https://static-cdn.jtvnw.net/ttv-boxart/" + encodeURIComponent(c.id) + "-{width}x{height}.jpg";
  return url.replace("{width}x{height}", "72x96").replace(/-\d+x\d+(\.\w+)$/, "-72x96$1");
}

function artImg(url) {
  const img = el("img");
  img.alt = "";
  img.loading = "lazy";
  img.referrerPolicy = "no-referrer";
  img.addEventListener("error", () => img.remove());
  img.src = url;
  return img;
}

/** The small box art next to the category field. */
function showCategoryArt(c) {
  const box = $("cat-art");
  const url = boxArt(c);
  if (box.dataset.url === String(url)) return;
  box.dataset.url = String(url);
  box.replaceChildren(icon("i-image"));
  if (url) box.appendChild(artImg(url));
}

function infoChanged() {
  if (!channelInfo) return false;
  const title = $("info-title-input").value.trim();
  const titleChanged = title !== "" && title !== channelInfo.title;
  const catChanged = !!pickedCategory && pickedCategory.id !== channelInfo.game_id;
  return titleChanged || catChanged;
}

function updateSaveButton() {
  const changed = infoChanged();
  $("info-save").disabled = !changed || savingInfo;
  $("info-state").textContent = savingInfo ? "Saving" : !channelInfo ? "Reading from Twitch" : changed ? "Not saved yet" : "Saved on Twitch";
  const n = $("info-title-input").value.length;
  const count = $("title-count");
  count.textContent = n + "/140";
  count.classList.toggle("near", n > 120);
}

function showChannelInfo(info) {
  channelInfo = info;
  pickedCategory = info.game_id ? { id: info.game_id, name: info.game_name } : null;
  $("info-title-input").value = info.title || "";
  $("info-cat-input").value = info.game_name || "";
  showCategoryArt(pickedCategory);
  fitTitle();
  updateSaveButton();
}

async function loadChannelInfo(quiet = false) {
  const connected = !!auth.twitch;
  $("info-connect").hidden = connected;
  $("info-form").hidden = !connected;
  if (!connected) {
    channelInfo = null;
    return;
  }
  updateSaveButton();
  try {
    const info = await invoke("get_channel_info");
    if (!info) return;
    // Mid-edit means an unsaved change, or a category search in progress.
    const searching = document.activeElement === $("info-cat-input") && !pickedCategory;
    if (searching || infoChanged()) {
      // Keep what is being typed; just compare against the latest values from Twitch.
      channelInfo = info;
      updateSaveButton();
    } else {
      showChannelInfo(info);
    }
  } catch (x) {
    if (!quiet) toast("Could not read your stream info: " + x, true);
  }
}

// Keep the fields in sync with Twitch, in case the title or category is changed elsewhere.
const INFO_POLL_MS = 30000;
setInterval(() => {
  if (auth.twitch && !document.hidden) loadChannelInfo(true);
}, INFO_POLL_MS);

$("info-connect-btn").addEventListener("click", openTwitchDialog);
/** The title box grows with the text, so the whole title stays readable. */
function fitTitle() {
  const t = $("info-title-input");
  if (!t.offsetParent) return;
  t.style.height = "auto";
  t.style.height = t.scrollHeight + "px";
}

$("info-title-input").addEventListener("input", () => {
  const t = $("info-title-input");
  // Twitch titles are one line, so line breaks (from typing or pasting) become spaces.
  if (/[\r\n]/.test(t.value)) t.value = t.value.replace(/[\r\n]+/g, " ");
  fitTitle();
  updateSaveButton();
});
$("info-title-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    $("info-form").requestSubmit();
  }
});
window.addEventListener("resize", fitTitle);

let catTimer = null;
let catRequest = 0;
let catResults = [];
let catActive = -1;

function closeCatList() {
  $("cat-list").hidden = true;
  $("info-cat-input").setAttribute("aria-expanded", "false");
  catActive = -1;
}

function pickCategory(c) {
  pickedCategory = { id: c.id, name: c.name, art: c.box_art_url };
  $("info-cat-input").value = c.name;
  showCategoryArt(pickedCategory);
  closeCatList();
  updateSaveButton();
}

function renderCatList() {
  const list = $("cat-list");
  if (!catResults.length) {
    list.replaceChildren(el("li", "cat-empty", "No categories found"));
  } else {
    list.replaceChildren(...catResults.map((c, i) => {
      const li = el("li", "cat-option" + (i === catActive ? " active" : ""));
      const art = el("span", "cat-art");
      art.appendChild(icon("i-image"));
      const url = boxArt(c);
      if (url) art.appendChild(artImg(url));
      li.append(art, el("span", "cat-name", c.name));
      li.setAttribute("role", "option");
      li.setAttribute("aria-selected", String(i === catActive));
      li.addEventListener("mousedown", (e) => {
        e.preventDefault();
        pickCategory(c);
      });
      return li;
    }));
  }
  list.hidden = false;
  $("info-cat-input").setAttribute("aria-expanded", "true");
}

$("info-cat-input").addEventListener("input", () => {
  // Typing clears the earlier pick until a category is chosen from the list.
  pickedCategory = null;
  updateSaveButton();
  clearTimeout(catTimer);
  const q = $("info-cat-input").value.trim();
  if (!q) {
    closeCatList();
    return;
  }
  catTimer = setTimeout(async () => {
    const id = ++catRequest;
    try {
      const results = await invoke("search_categories", { query: q });
      if (id !== catRequest) return;
      catResults = results;
      catActive = results.length ? 0 : -1;
      renderCatList();
    } catch (x) {
      if (id === catRequest) toast(String(x), true);
    }
  }, 250);
});

// Clicking into the category selects it, so typing starts a new search right away.
$("info-cat-input").addEventListener("focus", () => $("info-cat-input").select());

$("info-cat-input").addEventListener("keydown", (e) => {
  if ($("cat-list").hidden || !catResults.length) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const step = e.key === "ArrowDown" ? 1 : -1;
    catActive = (catActive + step + catResults.length) % catResults.length;
    renderCatList();
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (catActive >= 0) pickCategory(catResults[catActive]);
  } else if (e.key === "Escape") {
    e.stopPropagation();
    closeCatList();
  }
});

$("info-cat-input").addEventListener("blur", () => {
  clearTimeout(catTimer);
  catRequest++;
  closeCatList();
  // Leaving the box without picking puts the current category back.
  if (!pickedCategory && channelInfo) {
    pickedCategory = channelInfo.game_id ? { id: channelInfo.game_id, name: channelInfo.game_name } : null;
    $("info-cat-input").value = channelInfo.game_name || "";
    showCategoryArt(pickedCategory);
    updateSaveButton();
  }
});

$("info-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!infoChanged() || savingInfo) return;
  const title = $("info-title-input").value.trim();
  const args = {
    title: title !== channelInfo.title ? title : null,
    categoryId: pickedCategory && pickedCategory.id !== channelInfo.game_id ? pickedCategory.id : null,
  };
  savingInfo = true;
  updateSaveButton();
  try {
    const info = await invoke("update_channel", args);
    savingInfo = false;
    showChannelInfo(info);
    toast("Stream info updated");
  } catch (x) {
    toast(String(x), true);
  } finally {
    savingInfo = false;
    updateSaveButton();
  }
});

// ---------- Twitch connection ----------

let deviceUri = null;
let twitchFrom = null;

function openTwitchDialog() {
  twitchFrom = $("settings").hidden ? document.activeElement : $("settings-btn");
  closeSettings();
  if (!$("tw-client-id").value) $("tw-client-id").value = store("twitchClientId") || auth.defaultTwitchClientId || "";
  $("twitch-dialog").hidden = false;
  $("tw-client-id").focus();
}

async function cancelDeviceFlow() {
  if ($("tw-code").hidden) return;
  await invoke("twitch_cancel_sign_in").catch(() => {});
  $("tw-code").hidden = true;
  $("tw-connect").disabled = false;
}

async function closeTwitchDialog() {
  if ($("twitch-dialog").hidden) return;
  $("twitch-dialog").hidden = true;
  if (twitchFrom && twitchFrom.focus) twitchFrom.focus();
  await cancelDeviceFlow();
}

$("tw-connect-menu").addEventListener("click", openTwitchDialog);
$("twitch-close").addEventListener("click", closeTwitchDialog);

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
$("tw-cancel").addEventListener("click", (e) => {
  e.preventDefault();
  cancelDeviceFlow();
});

// ---------- ad breaks ----------

// Twitch's ad schedule says when the next break runs and when the last one started. It is
// checked every 30 seconds, and every 5 seconds around a break so the bar switches quickly.
// The countdown itself ticks locally every second.
const AD_SOON_S = 5 * 60;
let ads = null; // { schedule, needsReconnect }
let adTimer = null;
let adRun = 0;
// The length of each upcoming break as it was announced, by start time, because Twitch's
// `duration` describes the next break, not the one that is running.
const adLengths = new Map();

function nowSec() {
  return Date.now() / 1000;
}

function clock(s) {
  s = Math.max(0, Math.ceil(s));
  const m = Math.floor(s / 60);
  return m + ":" + String(s % 60).padStart(2, "0");
}

function minutes(s) {
  const m = Math.round(s / 60);
  if (m < 60) return m + " min";
  const h = Math.floor(m / 60);
  return h + " h" + (m % 60 ? " " + (m % 60) + " min" : "");
}

function timeAt(unix) {
  return new Date(unix * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** The break running right now, as { start, length }, or null. */
function runningAd(sch, now) {
  if (!sch.lastAdAt) return null;
  let length = 0;
  for (const [at, len] of adLengths) if (Math.abs(at - sch.lastAdAt) <= 90) length = len;
  length = length || sch.duration || 60;
  return now < sch.lastAdAt + length ? { start: sch.lastAdAt, length } : null;
}

function renderAd() {
  const bar = $("adbar");
  const show = (state, title, sub, time, progress) => {
    bar.hidden = false;
    bar.className = "adbar" + (state ? " " + state : "");
    if ($("ad-title").textContent !== title) $("ad-title").textContent = title;
    $("ad-sub").textContent = sub;
    $("ad-time").textContent = time;
    $("ad-progress").style.width = Math.max(0, Math.min(100, progress * 100)) + "%";
  };
  $("ad-reconnect").hidden = !(ads && ads.needsReconnect);
  $("ad-time").hidden = !!(ads && ads.needsReconnect);
  if (!auth.twitch || !ads) {
    bar.hidden = true;
    return;
  }
  if (ads.needsReconnect) {
    show("", "See ad breaks here", "Reconnect Twitch once so Sylva Control can read your ad schedule.", "", 0);
    return;
  }
  const sch = ads.schedule;
  const now = nowSec();
  const run = sch && runningAd(sch, now);
  if (run) {
    const left = run.start + run.length - now;
    show("running", "Ads are running", run.length + " second break. Back at " + timeAt(run.start + run.length) + ".",
      clock(left), (now - run.start) / run.length);
    return;
  }
  if (!sch || !sch.nextAdAt) {
    bar.hidden = true;
    return;
  }
  const until = sch.nextAdAt - now;
  const len = sch.duration ? sch.duration + " second break" : "Ad break";
  if (until <= 0) {
    show("running", "Ad break starting", len + ", starting now.", "0:00", 1);
  } else if (until <= AD_SOON_S) {
    show("soon", "Ad break soon", len + " at " + timeAt(sch.nextAdAt) + ".", clock(until), 1 - until / AD_SOON_S);
  } else {
    const snoozes = sch.snoozeCount ? ", " + sch.snoozeCount + (sch.snoozeCount === 1 ? " snooze" : " snoozes") + " left" : "";
    show("", "Next ad break", len + " at " + timeAt(sch.nextAdAt) + snoozes + ".", minutes(until), 0);
  }
}

async function refreshAds() {
  clearTimeout(adTimer);
  const run = ++adRun;
  if (!auth.twitch) {
    ads = null;
    renderAd();
    return;
  }
  let next = 30000;
  try {
    const s = await invoke("get_ad_schedule");
    if (run !== adRun) return;
    ads = { schedule: s.schedule, needsReconnect: s.needsReconnect };
    const sch = s.schedule;
    if (sch && sch.nextAdAt && sch.duration) adLengths.set(sch.nextAdAt, sch.duration);
    if (adLengths.size > 20) adLengths.delete(adLengths.keys().next().value);
    // A saved sign-in without the ads permission stays that way until Twitch is reconnected.
    if (s.needsReconnect) next = 0;
    else if (sch) {
      const now = nowSec();
      if (runningAd(sch, now) || (sch.nextAdAt && sch.nextAdAt - now <= 90)) next = 5000;
    }
  } catch {
    // Keep showing the last schedule; the next check usually works.
  }
  if (run !== adRun) return;
  renderAd();
  if (next) adTimer = setTimeout(refreshAds, next);
}

// The countdown ticks every second while the window can be seen.
setInterval(() => {
  if (ads && !document.hidden && !$("adbar").hidden) renderAd();
}, 1000);

$("ad-reconnect").addEventListener("click", openTwitchDialog);

// ---------- updates (installed copies and git checkouts) ----------

// Checked at startup and then every 15 minutes, with no action needed.
const UPDATE_CHECK_MS = 15 * 60 * 1000;
let updateTimer = null;
let announcedBehind = 0;
let updating = false;

async function checkUpdates(manual = false) {
  clearTimeout(updateTimer);
  updateTimer = setTimeout(checkUpdates, UPDATE_CHECK_MS);
  if (updating) return;
  const btn = $("update-btn");
  try {
    const u = await invoke("check_for_update");
    $("check-update").hidden = !u.supported;
    if (!u.supported) return;
    btn.hidden = u.behind === 0;
    if (u.behind > 0) {
      btn.title = "What's new:\n" + u.changes.map((c) => "- " + c).join("\n");
      // Say so once per new update, even when the check ran on its own.
      if (manual || u.behind !== announcedBehind) {
        toast("A Sylva Control update is ready. Click Update at the top to install it.", false, 10000);
      }
      announcedBehind = u.behind;
    } else if (manual) {
      toast("Sylva Control is up to date");
    }
  } catch (x) {
    if (manual) toast(String(x), true);
    else console.warn(x);
  }
}

const UPDATE_STEPS = { download: "Step 1 of 3: Downloading changes", build: "Step 2 of 3: Building", restart: "Step 3 of 3: Restarting" };

function showUpdateStatus(s) {
  closeSettings();
  $("update-dialog").hidden = false;
  const failed = s.step === "error";
  $("update-progress").hidden = failed;
  $("update-close").hidden = !failed;
  $("update-error").hidden = !failed;
  $("update-text").hidden = failed;
  if (failed) {
    $("update-step").textContent = "The update did not finish. Sylva Control is still on the version you had.";
    $("update-error").textContent = s.text;
    return;
  }
  if (s.step === "fetch" || (s.step === "restart" && s.text === "Installing")) {
    $("update-step").textContent = s.text;
    return;
  }
  let text = UPDATE_STEPS[s.step] || s.text;
  if (s.step === "build" && s.compiled > 0) text += " (" + s.compiled + (s.compiled === 1 ? " part" : " parts") + " done)";
  $("update-step").textContent = text;
}

$("update-btn").addEventListener("click", async () => {
  if (updating) return;
  updating = true;
  $("update-btn").disabled = true;
  showUpdateStatus({ step: "fetch", text: "Starting", compiled: 0 });
  try {
    await invoke("apply_update");
  } catch (x) {
    showUpdateStatus({ step: "error", text: String(x), compiled: 0 });
  }
});

$("update-close").addEventListener("click", () => {
  $("update-dialog").hidden = true;
  $("update-btn").disabled = false;
  updating = false;
});

$("check-update").addEventListener("click", () => checkUpdates(true));

// ---------- startup ----------

async function start() {
  renderChips();
  renderFeed();

  await listen("activity", (e) => {
    addLive(e.payload);
    refreshStatsSoon();
  });
  await listen("sylva-tips", () => loadHeldTips());
  await listen("sylva-status", (e) => {
    if (e.payload.state === "unauthorized") toast("Sylva no longer accepts Sylva Control. Sign out and sign in again.", true);
  });
  await listen("sylva-auth", async (e) => {
    const btn = $("signin-submit");
    btn.disabled = false;
    btn.textContent = "Sign in with Sylva";
    $("signin-code").hidden = true;
    if (e.payload.ok) {
      toast("Signed in" + (e.payload.name ? " as " + e.payload.name : ""));
      applyAuth(await invoke("restore_session"));
      await loadFeed();
      refreshStats();
      refreshViewers();
    } else {
      const err = $("signin-error");
      err.textContent = "Sign-in failed: " + e.payload.error;
      err.hidden = false;
    }
  });
  await listen("update-status", (e) => showUpdateStatus(e.payload));
  await listen("twitch-auth", async (e) => {
    $("tw-code").hidden = true;
    $("tw-connect").disabled = false;
    if (e.payload.ok) {
      $("twitch-dialog").hidden = true;
      toast("Twitch connected as " + e.payload.login);
      applyAuth(await invoke("restore_session"));
      refreshViewers();
      refreshStats();
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
  refreshViewers();
  setTimeout(checkUpdates, 3000);
}

start();
