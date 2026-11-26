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
  setTimeout(() => t.remove(), ms);
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

// ---------- menus ----------

const MENUS = [["account-btn", "account-menu"]];

function closeMenus(except) {
  for (const [b, m] of MENUS) {
    if (m === except) continue;
    $(m).hidden = true;
    $(b).setAttribute("aria-expanded", "false");
  }
}

for (const [b, m] of MENUS) {
  $(b).addEventListener("click", (e) => {
    e.stopPropagation();
    const open = $(m).hidden;
    closeMenus(m);
    $(m).hidden = !open;
    $(b).setAttribute("aria-expanded", String(open));
  });
  $(m).addEventListener("click", (e) => e.stopPropagation());
}
document.addEventListener("click", () => closeMenus());
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  closeMenus();
  if (!$("twitch-dialog").hidden) closeTwitchDialog();
});

// ---------- side rail ----------

const railBtns = [...document.querySelectorAll(".rail-btn[data-section]")];

function markRail() {
  const content = $("content");
  const top = content.getBoundingClientRect().top + 80;
  let current = railBtns[0];
  // At the very bottom the last section may be too short to reach the top.
  const atEnd = content.scrollTop + content.clientHeight >= content.scrollHeight - 4;
  for (const b of railBtns) {
    if ($(b.dataset.section).getBoundingClientRect().top <= top) current = b;
  }
  if (atEnd && content.scrollTop > 0) current = railBtns[railBtns.length - 1];
  for (const b of railBtns) b.classList.toggle("active", b === current);
}

for (const b of railBtns) {
  b.addEventListener("click", () => {
    $(b.dataset.section).scrollIntoView({ behavior: "smooth", block: "start" });
  });
}
$("content").addEventListener("scroll", markRail, { passive: true });

// ---------- auth ----------

let auth = { streamelements: null, twitch: null, defaultTwitchClientId: null };

function applyAuth(a) {
  auth = a;
  const se = a.streamelements;
  $("signin").hidden = !!se;
  const name = se ? se.displayName || se.username || "Streamer" : "";
  $("brand-channel").textContent = name;
  const av = avatar(name || "?", se && se.avatar);
  av.id = "user-avatar";
  $("user-avatar").replaceWith(av);
  $("menu-se-name").textContent = se ? name : "-";
  $("menu-tw-name").textContent = a.twitch ? a.twitch.login : "Not connected";
  $("tw-signout").hidden = !a.twitch;
  $("tw-connect-menu").hidden = !!a.twitch;
  $("se-signout").hidden = !se;
  loadChannelInfo();
  refreshAds();
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
    toast("Signed in as " + (a.streamelements.displayName || a.streamelements.username));
    await loadFeed();
    refreshStats();
    refreshViewers();
  } catch (x) {
    err.textContent = String(x);
    err.hidden = false;
  } finally {
    btn.disabled = false;
    btn.textContent = "Sign in";
  }
});

$("se-signout").addEventListener("click", async () => {
  closeMenus();
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
  closeMenus();
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

// ---------- realtime status ----------

function setRealtime(s) {
  // Cortex reconnects on its own, so only a rejected token needs the user's attention.
  if (s.state === "unauthorized") {
    toast("Live events stopped: " + (s.message || "token rejected") + ". Sign in again.", true);
  }
}

// ---------- activity feed ----------

const GROUPS = [
  { id: "follow", label: "Follows", color: "c-follow", icon: "i-heart", kinds: ["follow"] },
  { id: "sub", label: "Subs", color: "c-sub", icon: "i-star", kinds: ["subscriber"] },
  { id: "gift", label: "Gifts", color: "c-gift", icon: "i-gift", kinds: ["giftedSub", "communityGift"] },
  { id: "cheer", label: "Bits", color: "c-bits", icon: "i-gem", kinds: ["cheer"] },
  { id: "tip", label: "Tips", color: "c-tip", icon: "i-tip", kinds: ["tip"] },
  { id: "redemption", label: "Rewards", color: "c-reward", icon: "i-sparkles", kinds: ["redemption"] },
  { id: "raid", label: "Raids", color: "c-raid", icon: "i-swords", kinds: ["raid", "host"] },
  { id: "other", label: "Other", color: "c-other", icon: "i-dot", kinds: ["other"] },
];
const KNOWN_KINDS = new Set(GROUPS.flatMap((g) => g.kinds));
const MAX_ITEMS = 500;
const DAY = 86400000;

let feed = [];
// Groups the user picked. Empty means everything.
let shown = new Set((store("shownGroups") || []).filter((g) => GROUPS.some((x) => x.id === g)));

function groupOf(kind) {
  return GROUPS.find((g) => g.kinds.includes(kind)) || GROUPS[GROUPS.length - 1];
}

function last24(a) {
  return Date.now() - Date.parse(a.createdAt) < DAY;
}

/** Filter buttons with counts, and the colored mix bar for the last 24 hours. */
function renderSummary() {
  const counts = Object.fromEntries(GROUPS.map((g) => [g.id, 0]));
  let total = 0;
  for (const a of feed) {
    if (!last24(a)) continue;
    counts[groupOf(a.kind).id]++;
    total++;
  }

  const mix = $("mix");
  mix.classList.toggle("empty", total === 0);
  if (total === 0) {
    const s = el("span");
    s.style.flexGrow = "1";
    mix.replaceChildren(s);
  } else {
    mix.replaceChildren(
      ...GROUPS.filter((g) => counts[g.id] > 0).map((g) => {
        const s = el("span", g.color);
        s.style.flexGrow = String(counts[g.id]);
        s.title = num(counts[g.id]) + " " + g.label.toLowerCase();
        return s;
      })
    );
  }
  $("mix-label").textContent = total === 1 ? "1 event in the last 24 hours" : num(total) + " events in the last 24 hours";

  const all = el("button", "filter");
  all.type = "button";
  all.setAttribute("aria-pressed", String(shown.size === 0));
  all.append(document.createTextNode("All"), el("span", "count", num(total)));
  all.addEventListener("click", () => {
    shown.clear();
    store("shownGroups", []);
    renderFeed();
  });
  const buttons = [all];
  for (const g of GROUPS) {
    if (g.id === "other" && counts.other === 0 && !shown.has("other")) continue;
    const b = el("button", "filter " + g.color);
    b.type = "button";
    b.title = "Show " + g.label.toLowerCase() + " (click more than one to combine)";
    b.setAttribute("aria-pressed", String(shown.has(g.id)));
    b.append(el("span", "bar"), document.createTextNode(g.label), el("span", "count", num(counts[g.id])));
    b.addEventListener("click", () => {
      if (shown.has(g.id)) shown.delete(g.id);
      else shown.add(g.id);
      store("shownGroups", [...shown]);
      renderFeed();
    });
    buttons.push(b);
  }
  $("filters").replaceChildren(...buttons);
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
      b(a.sender || "Anonymous"); t(" gifted a sub to "); b(name);
      if (tier) pills.push(tier);
      // StreamElements fills in an automatic "X gifted a sub to Y" message; the sentence already says it.
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
    default:
      b(name);
      if (a.rawType) pills.push(a.rawType);
  }
  return { parts, pills, message };
}

function renderItem(a, fresh) {
  const g = groupOf(a.kind);
  const d = describe(a);
  const kind = KNOWN_KINDS.has(a.kind) ? a.kind : "other";
  const li = el("li", "item k-" + kind + (fresh ? " fresh" : ""));
  li.dataset.id = a.id;

  const tile = el("span", "tile");
  tile.appendChild(icon(g.icon));

  const main = el("div", "item-main");
  const title = el("div", "item-title");
  title.append(...d.parts);
  main.appendChild(title);
  if (d.message) main.appendChild(el("p", "item-msg", d.message));
  if (d.pills.length) {
    const pills = el("div", "pills");
    for (const p of d.pills) pills.appendChild(el("span", "pill", p));
    main.appendChild(pills);
  }

  const side = el("div", "item-side");
  const time = el("time", null, ago(a.createdAt));
  if (a.createdAt) {
    time.dateTime = a.createdAt;
    time.title = new Date(a.createdAt).toLocaleString();
  }
  side.appendChild(time);

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

  li.append(tile, main, side, replay);
  return li;
}

function visible(a) {
  return !shown.size || shown.has(groupOf(a.kind).id);
}

function renderFeed() {
  const items = feed.filter(visible);
  $("feed").replaceChildren(...items.map((a) => renderItem(a, false)));
  const empty = $("feed-empty");
  empty.hidden = items.length > 0;
  empty.textContent = feed.length
    ? "Nothing here for this filter yet."
    : "No activity yet. Follows, subs, bits and rewards appear here the moment they happen.";
  renderSummary();
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
  renderSummary();
}

async function loadFeed() {
  if (!auth.streamelements) return;
  const btn = $("feed-refresh");
  btn.classList.add("spin");
  try {
    const history = await invoke("get_feed", { limit: 250 });
    const known = new Set(history.map((a) => a.id));
    // Keep live items that arrived while history was loading.
    feed = feed.filter((a) => !known.has(a.id)).concat(history).slice(0, MAX_ITEMS);
    renderFeed();
  } catch (x) {
    toast("Could not load activity: " + x, true);
  } finally {
    btn.classList.remove("spin");
  }
}

$("feed-refresh").addEventListener("click", loadFeed);

setInterval(() => {
  for (const t of document.querySelectorAll("#feed time[datetime]")) t.textContent = ago(t.dateTime);
  renderSummary();
}, 30000);

// ---------- numbers ----------

function resetStats() {
  for (const id of ["stat-viewers", "stat-followers", "stat-subs", "stat-followers-session", "stat-subs-session"]) {
    $(id).textContent = "-";
    $(id).classList.remove("up");
  }
  setViewerNote(null);
}

function setSession(id, n) {
  const e = $(id);
  e.textContent = n === null || n === undefined ? "-" : "+" + num(n);
  e.classList.toggle("up", !!n);
  e.title = "New since the stream started";
}

let statsTimer = null;
async function refreshStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(refreshStats, 60000);
  if (!auth.streamelements) return;
  try {
    const s = await invoke("get_stats");
    $("stat-followers").textContent = num(s.followers);
    $("stat-subs").textContent = num(s.subscribers);
    setSession("stat-followers-session", s.followersSession);
    setSession("stat-subs-session", s.subscribersSession);
  } catch (x) {
    console.warn(x);
  }
}

let statsSoon = null;
function refreshStatsSoon() {
  clearTimeout(statsSoon);
  statsSoon = setTimeout(refreshStats, 3000);
}

/** A short muted line under the viewer count, only when the count cannot be shown. */
function setViewerNote(text, detail) {
  const note = $("viewer-note");
  note.hidden = !text;
  note.textContent = text || "";
  note.title = detail || "";
}

// Viewers change all the time, so they are checked often. No Twitch sign-in is needed.
// With Twitch connected the count comes from the official API, which is cheap to ask every
// 5 seconds. Without it, the public lookup is asked every 10 seconds to stay polite.
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
      n.title = "";
      setViewerNote(auth.streamelements ? "No Twitch channel on this StreamElements account" : null);
      return;
    }
    n.textContent = v.live ? num(v.viewers) : "0";
    n.title = "Checked at " + new Date().toLocaleTimeString();
    setViewerNote(null);
  } catch (x) {
    $("stat-viewers").textContent = "-";
    setViewerNote("Viewer count unavailable", String(x));
  } finally {
    // Only the latest check schedules the next one, so checks never pile up.
    if (run === viewerRun) viewerTimer = setTimeout(refreshViewers, VIEWER_POLL_MS[viewerSource] || 10000);
  }
}

// Refresh right away when the window comes back into view.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) return;
  refreshViewers();
  if (auth.twitch) loadChannelInfo(true);
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

/** A button that switches between two alert actions, such as mute and unmute. */
function toggleButton(btn, actions, labels, icons, toasts) {
  btn.addEventListener("click", async () => {
    const on = btn.getAttribute("aria-pressed") === "true";
    const ok = await runAction(btn, () => invoke("alert_action", { action: on ? actions[1] : actions[0] }), on ? toasts[1] : toasts[0]);
    if (!ok) return;
    const now = !on;
    btn.setAttribute("aria-pressed", String(now));
    btn.querySelector("span").textContent = now ? labels[1] : labels[0];
    btn.querySelector("use").setAttribute("href", "#" + (now ? icons[1] : icons[0]));
  });
}

toggleButton($("act-mute"), ["mute", "unmute"], ["Mute alerts", "Unmute alerts"], ["i-vol", "i-volx"], ["Alerts muted", "Alerts unmuted"]);
toggleButton($("act-pause"), ["pause", "unpause"], ["Pause alerts", "Resume alerts"], ["i-pause", "i-play"], ["Alerts paused", "Alerts resumed"]);
$("act-skip").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("alert_action", { action: "skip" }), "Skipped the current alert"));
$("act-reload").addEventListener("click", (e) =>
  runAction(e.currentTarget, () => invoke("reload_overlays"), "Overlays reloaded"));

// ---------- stream title and category ----------

// What Twitch has now, so Save only sends what changed.
let channelInfo = null;
// The category picked from the list: { id, name }.
let pickedCategory = null;

function infoChanged() {
  if (!channelInfo) return false;
  const title = $("info-title-input").value.trim();
  const titleChanged = title !== "" && title !== channelInfo.title;
  const catChanged = !!pickedCategory && pickedCategory.id !== channelInfo.game_id;
  return titleChanged || catChanged;
}

function updateSaveButton() {
  $("info-save").disabled = !infoChanged();
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
$("info-title-input").addEventListener("input", updateSaveButton);

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
  pickedCategory = { id: c.id, name: c.name };
  $("info-cat-input").value = c.name;
  closeCatList();
  updateSaveButton();
}

function renderCatList() {
  const list = $("cat-list");
  if (!catResults.length) {
    list.replaceChildren(el("li", "cat-empty", "No categories found"));
  } else {
    list.replaceChildren(...catResults.map((c, i) => {
      const li = el("li", "cat-option" + (i === catActive ? " active" : ""), c.name);
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
  closeCatList();
  // Leaving the box without picking puts the current category back.
  if (!pickedCategory && channelInfo) {
    pickedCategory = channelInfo.game_id ? { id: channelInfo.game_id, name: channelInfo.game_name } : null;
    $("info-cat-input").value = channelInfo.game_name || "";
    updateSaveButton();
  }
});

$("info-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!infoChanged()) return;
  const title = $("info-title-input").value.trim();
  const args = {
    title: title !== channelInfo.title ? title : null,
    categoryId: pickedCategory && pickedCategory.id !== channelInfo.game_id ? pickedCategory.id : null,
  };
  const btn = $("info-save");
  btn.disabled = true;
  btn.textContent = "Saving...";
  try {
    showChannelInfo(await invoke("update_channel", args));
    toast("Stream info updated");
  } catch (x) {
    toast(String(x), true);
  } finally {
    btn.textContent = "Save";
    updateSaveButton();
  }
});

// ---------- Twitch connection ----------

let deviceUri = null;

function openTwitchDialog() {
  closeMenus();
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
  $("twitch-dialog").hidden = true;
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
  if (!auth.twitch || !ads) {
    bar.hidden = true;
    return;
  }
  if (ads.needsReconnect) {
    show("", "See ad breaks here", "Reconnect Twitch once so Cortex can read your ad schedule.", "", 0);
    return;
  }
  const sch = ads.schedule;
  const now = nowSec();
  const run = sch && runningAd(sch, now);
  if (run) {
    const left = run.start + run.length - now;
    show("running", "Ads are running", run.length + " second break. Back at " + timeAt(run.start + run.length) + ".",
      clock(left) + " left", (now - run.start) / run.length);
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
    show("", "Next ad break", len + " at " + timeAt(sch.nextAdAt) + snoozes + ".", "in " + minutes(until), 0);
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

setInterval(() => {
  if (ads && !$("adbar").hidden) renderAd();
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
        toast("A Cortex update is ready. Click Update at the top to install it.", false, 10000);
      }
      announcedBehind = u.behind;
    } else if (manual) {
      toast("Cortex is up to date");
    }
  } catch (x) {
    if (manual) toast(String(x), true);
    else console.warn(x);
  }
}

const UPDATE_STEPS = { download: "Step 1 of 3: Downloading changes", build: "Step 2 of 3: Building", restart: "Step 3 of 3: Restarting" };

function showUpdateStatus(s) {
  $("update-dialog").hidden = false;
  const failed = s.step === "error";
  $("update-progress").hidden = failed;
  $("update-close").hidden = !failed;
  $("update-error").hidden = !failed;
  $("update-text").hidden = failed;
  if (failed) {
    $("update-step").textContent = "The update did not finish. Cortex is still on the version you had.";
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

$("check-update").addEventListener("click", () => {
  closeMenus();
  checkUpdates(true);
});

// ---------- startup ----------

async function start() {
  renderFeed();

  await listen("activity", (e) => {
    addLive(e.payload);
    refreshStatsSoon();
  });
  await listen("realtime-status", (e) => setRealtime(e.payload));
  await listen("update-status", (e) => showUpdateStatus(e.payload));
  await listen("twitch-auth", async (e) => {
    $("tw-code").hidden = true;
    $("tw-connect").disabled = false;
    if (e.payload.ok) {
      $("twitch-dialog").hidden = true;
      toast("Twitch connected as " + e.payload.login);
      applyAuth(await invoke("restore_session"));
      refreshViewers();
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
