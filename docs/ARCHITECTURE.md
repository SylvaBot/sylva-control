# Cuepoint architecture

Cuepoint is a desktop control room for streamers who use StreamElements: a live activity
feed, alert quick actions, and an audience panel (subscribers, followers, viewers, chatters).

## Stack and why

| Layer | Choice | Reason |
| --- | --- | --- |
| App shell | Tauri 2 (Rust) | Small native binary that uses the system webview (WebView2 on Windows). All networking, secrets and state live in Rust. |
| UI | Plain HTML, CSS and JavaScript in `ui/` | No Node toolchain or bundler needed. The UI only renders and calls Rust commands. |
| HTTP | reqwest with the OS TLS stack | Uses the Windows and macOS certificate stores, so corporate proxies and antivirus TLS work. |
| Realtime | tokio-tungstenite | The StreamElements Astro gateway is a plain JSON websocket. |
| Secrets | keyring | Tokens go in Windows Credential Manager, macOS Keychain or Linux Secret Service. Never in files. |

egui or iced were considered. Tauri won because a feed of rich, variable length messages,
avatars and badges is far easier to lay out well in HTML, and the Rust core stays GUI free
and testable either way.

## Layout of the code

```
crates/cuepoint-core/       Pure Rust library, no GUI code, fully unit tested
  src/activity.rs           One Activity model for REST history and live events
  src/se/mod.rs             StreamElements REST: channels/me, activities, sessions,
                            alert actions, overlay reload, activity replay
  src/se/astro.rs           Astro websocket client with backoff and reconnect tokens
  src/twitch.rs             Twitch device code sign-in and Helix lists
src-tauri/                  The desktop app
  src/commands.rs           Commands the UI calls with invoke()
  src/state.rs              Signed-in sessions, realtime task wiring
  src/secrets.rs            OS credential store
ui/                         index.html, styles.css, app.js
```

## Data sources

| Panel | Source | Endpoint |
| --- | --- | --- |
| Activity history | StreamElements | `GET kappa/v2/activities/{channel}` (last 30 days, 100 items) |
| Live activity | StreamElements Astro | `wss://astro.streamelements.com`, topic `channel.activities` |
| Followers and subs totals | StreamElements | `GET kappa/v2/sessions/{channel}` (`follower-total`, `subscriber-total`) |
| Mute, pause, skip alerts | StreamElements | `PUT kappa/v3/overlays/{channel}/action` with `{"action": ...}` |
| Reload overlays | StreamElements | `PUT kappa/v2/overlays/{channel}/reload` |
| Replay an alert | StreamElements | `POST kappa/v2/activities/{channel}/{id}/replay` |
| Viewers, live state | Twitch Helix | `GET /streams` (`viewer_count`) |
| Chatters | Twitch Helix | `GET /chat/chatters` (scope `moderator:read:chatters`) |
| Subscriber list | Twitch Helix | `GET /subscriptions` (scope `channel:read:subscriptions`) |
| Follower list | Twitch Helix | `GET /channels/followers` (scope `moderator:read:followers`) |

StreamElements does not offer viewer or chatter lists, which is why Twitch is a second,
optional sign-in. Without it, the feed, quick actions and the follower and sub totals still work.

Sources: the official Astro docs at docs.streamelements.com/websockets and the community
REST reference at github.com/c4ldas/streamelements-api (api.yaml). The Twitch calls follow
dev.twitch.tv/docs.

## Authentication

* **StreamElements:** the user pastes the channel JWT from the dashboard
  (Account, Channels, Show secrets). It is checked with `GET channels/me`, then stored in the OS
  credential store. REST calls send `Authorization: Bearer <jwt>`; Astro gets it in the
  subscribe frame with `token_type: "jwt"`. StreamElements OAuth2 is a drop in replacement
  later (Astro accepts `token_type: "oauth2"`), but it requires applying to StreamElements for
  a client ID, so the JWT is the right starting point.
* **Twitch:** device code flow with a Public client, so no client secret ships in the app.
  The user registers a free app at dev.twitch.tv and pastes its Client ID once (or it can be
  baked in at build time with `CUEPOINT_TWITCH_CLIENT_ID`). Access tokens refresh
  automatically on a 401 and the new tokens are saved back to the credential store.

## Realtime flow

1. Connect to Astro, send `subscribe` for `channel.activities` in the channel's room.
2. On `response` without `error`, the status pill shows "Realtime on".
3. Each `message` frame is parsed into an `Activity` and emitted to the window as `activity`.
4. Websocket pings are answered automatically. If the server sends `reconnect`, the client
   reconnects with the `reconnect_token` and does not re-subscribe, as the protocol requires.
5. Unexpected drops retry with exponential backoff from 1 to 30 seconds.
6. `err_unauthorized` stops the loop and asks the user to sign in again.

## Testing

`cargo test --workspace` runs 15 tests, including a local websocket server that checks the
subscribe frame, ping and pong, activity delivery and the reconnect token flow, and a local
HTTP server that checks every REST request's method, path, auth header and body, the Twitch
token refresh and the device code polling.

## Roadmap

1. **Now:** sign-in, live feed with filters, quick actions, stats header, audience lists.
2. **Next:** read the current mute and pause state at startup (the Astro overlay topics are
   the likely source), desktop notifications for selected event types, minimum amount
   filters (bits, tips, months), and a compact always on top mode.
3. **Later:** StreamElements OAuth2 once a client ID is approved, a "play next" control if a
   supported endpoint exists, signed installers and auto update through Tauri's updater.
