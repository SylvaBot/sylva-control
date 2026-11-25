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

function toast(text, bad = false) {
  const t = el("div", "toast" + (bad ? " bad" : ""), text);
  $("toasts").appendChild(t);
  setTimeout(() => t.remove(), bad ? 6500 : 3000);
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

const MONO_COLORS = ["c-mint", "c-lavender", "c-pink", "c-sky", "c-butter", "c-coral", "c-sand"];
function monoColor(name) {
  let h = 0;
  for (const ch of name || "") h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return MONO_COLORS[h % MONO_COLORS.length];
}

/** Round avatar: the image when we have a safe URL, otherwise the first letter. */
function avatar(name, url, colorClass) {
  const a = el("span", "avatar " + (colorClass || monoColor(name)));
  const letter = (name || "?").trim().charAt(0).toUpperCase() || "?";
  const src = safeImg(url);
  if (src) {
    const img = el("img");
    img.src = src;
    img.alt = "";
    img.loading = "lazy";
    img.referrerPolicy = "no-referrer";
    img.onerror = () => { img.remove(); a.textContent = letter; };
    a.appendChild(img);
  } else {
    a.textContent = letter;
  }
  return a;
}

// ---------- auth ----------

let auth = { streamelements: null, twitch: null, defaultTwitchClientId: null };

function applyAuth(a) {
  auth = a;
  const se = a.streamelements;
  $("signin").hidden = !!se;
  const name = se ? se.displayName || se.username || "Streamer" : "Not signed in";
  $("user-name").textContent = name;
  $("user-avatar").replaceWith(Object.assign(avatar(name, se && se.avatar, "c-mint"), { id: "user-avatar" }));
  $("menu-se-name").textContent = se ? name : "-";
  $("menu-tw-name").textContent = a.twitch ? a.twitch.login : "Not connected";
  $("tw-signout").hidden = !a.twitch;
  $("se-signout").hidden = !se;
  $("aud-connect").hidden = !!a.twitch;
  $("aud-body").hidden = !a.twitch;
  if (!a.twitch) {
    const saved = store("twitchClientId");
    $("tw-client-id").value = saved || a.defaultTwitchClientId || "";
  }
  if (!se) setRealtime({ state: "stopped" });
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

function setMenu(open) {
  $("account-menu").hidden = !open;
  $("account-btn").setAttribute("aria-expanded", String(open));
}
$("account-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  setMenu($("account-menu").hidden);
});
document.addEventListener("click", (e) => {
  if (!$("account-menu").contains(e.target)) setMenu(false);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") setMenu(false);
});

$("se-signout").addEventListener("click", async () => {
  setMenu(false);
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
  setMenu(false);
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
  const text = p.querySelector(".status-text");
  p.classList.remove("ok", "warn", "bad");
  p.title = "Live events";
  switch (s.state) {
    case "connecting":
      text.textContent = "Connecting";
      p.classList.add("warn");
      break;
    case "connected":
      text.textContent = "Subscribing";
      p.classList.add("warn");
      break;
    case "live":
      text.textContent = "Live events on";
      p.classList.add("ok");
      break;
    case "reconnecting":
      text.textContent = "Reconnecting in " + s.inSecs + "s";
      p.title = s.reason || "";
      p.classList.add("warn");
      break;
    case "unauthorized":
      text.textContent = "Live events off";
      p.title = s.message || "";
      p.classList.add("bad");
      toast("Live events stopped: " + (s.message || "token rejected") + ". Sign in again.", true);
      break;
    default:
      text.textContent = "Offline";
  }
}

// ---------- activity feed ----------

const GROUPS = [
  { id: "follow", label: "Follows", color: "c-mint", icon: "i-heart", kinds: ["follow"] },
  { id: "sub", label: "Subs", color: "c-lavender", icon: "i-star", kinds: ["subscriber"] },
  { id: "gift", label: "Gifts", color: "c-pink", icon: "i-gift", kinds: ["giftedSub", "communityGift"] },
  { id: "cheer", label: "Bits", color: "c-sky", icon: "i-gem", kinds: ["cheer"] },
  { id: "tip", label: "Tips", color: "c-butter", icon: "i-coin", kinds: ["tip"] },
  { id: "redemption", label: "Rewards", color: "c-coral", icon: "i-bolt", kinds: ["redemption"] },
  { id: "raid", label: "Raids", color: "c-sand", icon: "i-flag", kinds: ["raid", "host"] },
  { id: "other", label: "Other", color: "c-slate", icon: "i-dot", kinds: ["other"] },
];
const MAX_ITEMS = 500;

let feed = [];
// Groups the user picked. Empty means "All".
let shown = new Set((store("shownGroups") || []).filter((g) => GROUPS.some((x) => x.id === g)));
let query = "";

function groupOf(kind) {
  return GROUPS.find((g) => g.kinds.includes(kind)) || GROUPS[GROUPS.length - 1];
}

function buildChips() {
  const wrap = $("feed-chips");
  wrap.replaceChildren();
  const all = el("button", "chip all");
  all.type = "button";
  all.setAttribute("aria-pressed", String(shown.size === 0));
  all.append(icon("i-grid"), document.createTextNode("All"));
  all.addEventListener("click", () => {
    shown.clear();
    store("shownGroups", []);
    buildChips();
    renderFeed();
  });
  wrap.appendChild(all);
  for (const g of GROUPS) {
    const c = el("button", "chip " + g.color);
    c.type = "button";
    c.setAttribute("aria-pressed", String(shown.has(g.id)));
    c.append(icon(g.icon), document.createTextNode(g.label));
    c.addEventListener("click", () => {
      if (shown.has(g.id)) shown.delete(g.id);
      else shown.add(g.id);
      store("shownGroups", [...shown]);
      buildChips();
      renderFeed();
    });
    wrap.appendChild(c);
  }
}

function describe(a) {
  // who, to, kind label, optional big headline {num, unit}, optional caption parts, extra sub boxes
  const name = a.displayName || a.username || "Someone";
  const tier = tierLabel(a.tier);
  const out = { who: name, to: null, kind: "", headline: null, caption: null, extra: [] };
  const plural = (n, one, many) => (n === 1 ? one : many);
  switch (a.kind) {
    case "follow":
      out.kind = "Follow";
      out.caption = ["Started following"];
      break;
    case "subscriber":
      out.kind = "Subscription";
      if (a.amount != null && a.amount > 1) {
        out.headline = { num: num(a.amount), unit: "months" };
      } else {
        out.caption = ["New subscriber"];
      }
      if (tier) out.extra.push(["Tier", tier.replace("Tier ", "")]);
      break;
    case "giftedSub":
      out.kind = "Gift";
      out.who = a.sender || "Anonymous";
      out.to = name;
      out.caption = tier ? ["Gifted a ", { b: tier }, " sub"] : ["Gifted a sub"];
      break;
    case "communityGift":
      out.kind = "Gift subs";
      if (a.amount != null) out.headline = { num: num(a.amount), unit: plural(a.amount, "gift sub", "gift subs") };
      break;
    case "cheer":
      out.kind = "Bits";
      if (a.amount != null) out.headline = { num: num(a.amount), unit: plural(a.amount, "bit", "bits") };
      break;
    case "tip":
      out.kind = "Tip";
      if (a.amount != null) out.headline = { num: money(a.amount, a.currency), unit: "" };
      break;
    case "redemption":
      out.kind = "Reward";
      out.caption = a.redemption ? ["Redeemed ", { b: a.redemption }] : ["Redeemed a reward"];
      break;
    case "raid":
      out.kind = "Raid";
      if (a.amount != null) out.headline = { num: num(a.amount), unit: plural(a.amount, "raider", "raiders") };
      break;
    case "host":
      out.kind = "Host";
      if (a.amount != null) out.headline = { num: num(a.amount), unit: plural(a.amount, "viewer", "viewers") };
      break;
    default:
      out.kind = a.rawType;
  }
  return out;
}

function renderItem(a, fresh) {
  const g = groupOf(a.kind);
  const d = describe(a);
  const li = el("li", "item k-" + a.kind + " " + g.color + (fresh ? " fresh" : ""));
  li.dataset.id = a.id;

  const top = el("div", "item-top");
  const pill = el("span", "name-pill");
  pill.appendChild(avatar(d.who, d.to ? null : a.avatar, g.color));
  const names = el("span", "names", d.who);
  if (d.to) {
    names.appendChild(el("span", "to", " to "));
    names.appendChild(document.createTextNode(d.to));
  }
  pill.appendChild(names);
  const kind = el("span", "kind", d.kind);
  const time = el("time", null, ago(a.createdAt));
  if (a.createdAt) {
    time.dateTime = a.createdAt;
    time.title = new Date(a.createdAt).toLocaleString();
  }
  const dot = el("span", "icon-dot " + g.color);
  dot.appendChild(icon(g.icon));
  top.append(pill, kind, el("span", "spacer"), time, dot);
  li.appendChild(top);

  if (d.headline) {
    const h = el("div", "headline");
    h.appendChild(el("span", "num", d.headline.num));
    if (d.headline.unit) h.appendChild(el("span", "unit", d.headline.unit));
    li.appendChild(h);
  }
  if (d.caption) {
    const c = el("p", "caption");
    for (const part of d.caption) {
      c.appendChild(typeof part === "string" ? document.createTextNode(part) : el("b", null, part.b));
    }
    li.appendChild(c);
  }
  // Gifted subs carry an auto-generated "X gifted a sub to Y" message that repeats the caption.
  const autoGiftText = a.kind === "giftedSub" && /gift/i.test(a.message || "");
  if (a.message && !autoGiftText) li.appendChild(el("p", "msg", a.message));
  if (d.extra.length) {
    const meta = el("div", "meta");
    for (const [k, v] of d.extra) {
      const s = el("div", "sub");
      s.append(el("span", null, k), el("b", null, v));
      meta.appendChild(s);
    }
    li.appendChild(meta);
  }

  const replay = el("button", "replay");
  replay.type = "button";
  replay.title = "Play this alert again";
  replay.append(icon("i-replay"), document.createTextNode("Replay"));
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
  if (shown.size && !shown.has(groupOf(a.kind).id)) return false;
  if (!query) return true;
  const hay = [a.displayName, a.username, a.sender, a.message, a.redemption].filter(Boolean).join(" ").toLowerCase();
  return hay.includes(query);
}

function renderFeed() {
  const items = feed.filter(visible);
  $("feed").replaceChildren(...items.map((a) => renderItem(a, false)));
  const empty = $("feed-empty");
  empty.hidden = items.length > 0;
  empty.textContent = feed.length
    ? "Nothing matches this filter."
    : "No activity yet. Follows, subs, bits and rewards will appear here as they happen.";
  renderChart();
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
  renderChart();
}

async function loadFeed() {
  if (!auth.streamelements) return;
  const btn = $("feed-refresh");
  btn.classList.add("spin");
  try {
    const history = await invoke("get_feed", { limit: 250 });
    const known = new Set(history.map((a) => a.id));
    // Keep live items that arrived while history was loading.
    feed = feed.filter((a) => !known.has(a.id)).concat(history);
    renderFeed();
  } catch (x) {
    toast("Could not load activity: " + x, true);
  } finally {
    btn.classList.remove("spin");
  }
}

$("feed-refresh").addEventListener("click", loadFeed);

let searchTimer = null;
$("feed-search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    query = $("feed-search").value.trim().toLowerCase();
    renderFeed();
  }, 120);
});

setInterval(() => {
  for (const t of document.querySelectorAll("#feed time[datetime]")) t.textContent = ago(t.dateTime);
  renderChart();
}, 60000);

// ---------- last 12 hours chart ----------

function renderChart() {
  const HOURS = 12;
  const now = new Date();
  const top = new Date(now);
  top.setMinutes(0, 0, 0); // start of the current hour
  const start = top.getTime() - (HOURS - 1) * 3600000;
  const counts = new Array(HOURS).fill(0);
  let follows = 0, subs = 0, bits = 0;
  for (const a of feed) {
    const t = Date.parse(a.createdAt);
    if (!(t >= start)) continue;
    const i = Math.min(HOURS - 1, Math.floor((t - start) / 3600000));
    counts[i]++;
    if (a.kind === "follow") follows++;
    else if (a.kind === "subscriber" || a.kind === "giftedSub" || a.kind === "communityGift") subs++;
    else if (a.kind === "cheer" && a.amount) bits += a.amount;
  }
  const max = Math.max(1, ...counts);
  const bars = $("bars");
  if (bars.children.length !== HOURS) {
    bars.replaceChildren(...counts.map(() => {
      const b = el("div", "bar");
      b.append(el("i"), el("span"));
      return b;
    }));
  }
  counts.forEach((c, i) => {
    const b = bars.children[i];
    b.classList.toggle("now", i === HOURS - 1);
    b.querySelector("i").style.height = Math.max(8, Math.round((c / max) * 100)) + "%";
    b.title = c + (c === 1 ? " event" : " events");
    const hour = new Date(start + i * 3600000);
    b.querySelector("span").textContent = i % 2 === 1 ? hour.toLocaleTimeString([], { hour: "numeric" }).replace(/\s/g, "") : "";
  });
  $("events-total").textContent = num(counts.reduce((s, c) => s + c, 0));
  $("sum-follows").textContent = num(follows);
  $("sum-subs").textContent = num(subs);
  $("sum-bits").textContent = num(bits);
}

// ---------- stats ----------

function resetStats() {
  for (const id of ["stat-viewers", "stat-chatters", "stat-followers", "stat-subs", "sub-status", "sub-updated", "sub-followers", "sub-subs"]) {
    $(id).textContent = "-";
  }
  $("live-pill").hidden = true;
}

function plusCount(n) {
  return n === null || n === undefined ? "-" : "+" + num(n);
}

let statsTimer = null;
async function refreshStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(refreshStats, 30000);
  if (!auth.streamelements && !auth.twitch) return;
  const btn = $("stats-refresh");
  btn.classList.add("spin");
  try {
    const s = await invoke("get_stats");
    $("stat-followers").textContent = num(s.followers);
    $("stat-subs").textContent = num(s.subscribers);
    $("sub-followers").textContent = plusCount(s.followersSession);
    $("sub-subs").textContent = plusCount(s.subscribersSession);
    $("stat-viewers").textContent = num(s.viewers);
    $("stat-chatters").textContent = num(s.chatters);
    const twitch = s.live !== null && s.live !== undefined;
    $("sub-status").textContent = twitch ? (s.live ? "Live" : "Offline") : "Connect Twitch";
    $("sub-updated").textContent = twitch ? new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "Connect Twitch";
    const live = $("live-pill");
    live.hidden = !twitch;
    live.textContent = s.live ? "Live" : "Offline";
    live.classList.toggle("on", !!s.live);
    if (s.problems.length) console.warn(s.problems.join("\n"));
  } catch (x) {
    console.warn(x);
  } finally {
    btn.classList.remove("spin");
  }
}
$("stats-refresh").addEventListener("click", refreshStats);

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
      btn.querySelector(".action-label").textContent = on ? offLabel : onLabel;
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
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const mm = String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0");
  $("timer-display").textContent = h > 0 ? h + ":" + mm : mm;
}
function timerStop() {
  if (timer.running) timer.banked += Date.now() - timer.startedAt;
  timer.running = false;
  clearInterval(timer.handle);
  $("timer-start").textContent = "Start";
  $("timer").classList.remove("running");
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
    $("timer").classList.add("running");
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

for (const tab of document.querySelectorAll(".seg")) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll(".seg")) t.setAttribute("aria-selected", String(t === tab));
    audKind = tab.dataset.kind;
    loadAudience(true);
  });
}

function renderAudience() {
  const q = $("aud-search").value.trim().toLowerCase();
  const rows = audItems.filter((u) => !q || u.user_name.toLowerCase().includes(q) || u.user_login.includes(q));
  $("aud-list").replaceChildren(
    ...rows.map((u) => {
      const li = el("li");
      li.append(avatar(u.user_name, null), el("span", "who", u.user_name));
      let meta = "";
      if (audKind === "subscribers") {
        meta = tierLabel(u.tier) || u.tier;
        if (u.is_gift) meta += u.gifter_name ? ", gift from " + u.gifter_name : ", gift";
      } else if (audKind === "followers" && u.followed_at) {
        meta = new Date(u.followed_at).toLocaleDateString([], { year: "numeric", month: "short", day: "numeric" });
      }
      if (meta) li.appendChild(el("span", "meta-pill", meta));
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
  $("aud-refresh").classList.add("spin");
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
      $("aud-refresh").classList.remove("spin");
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

// ---------- updates (git checkout installs) ----------

let updateTimer = null;
async function checkUpdates(manual = false) {
  clearTimeout(updateTimer);
  updateTimer = setTimeout(checkUpdates, 15 * 60 * 1000);
  const btn = $("update-btn");
  try {
    const u = await invoke("check_for_update");
    $("check-update").hidden = !u.supported;
    if (!u.supported) return;
    btn.hidden = u.behind === 0;
    if (u.behind > 0) {
      btn.textContent = u.behind === 1 ? "Update ready" : "Update ready (" + u.behind + ")";
      btn.title = "What's new:\n" + u.changes.map((c) => "- " + c).join("\n");
      if (manual) toast(u.behind === 1 ? "1 update is ready" : u.behind + " updates are ready");
    } else if (manual) {
      toast("Pine is up to date");
    }
  } catch (x) {
    if (manual) toast(String(x), true);
    else console.warn(x);
  }
}

$("update-btn").addEventListener("click", async () => {
  const btn = $("update-btn");
  btn.disabled = true;
  toast("Updating. Pine will close, rebuild and reopen in a minute or two.");
  try {
    await invoke("apply_update");
  } catch (x) {
    btn.disabled = false;
    toast(String(x), true);
  }
});

$("check-update").addEventListener("click", () => {
  setMenu(false);
  checkUpdates(true);
});

// ---------- startup ----------

async function start() {
  buildChips();
  renderFeed();
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
  setTimeout(checkUpdates, 3000);
}

start();
