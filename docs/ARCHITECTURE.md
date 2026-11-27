# Sylva Desktop architecture

Sylva Desktop is the desktop control room for Sylva: a live activity feed, alert quick actions,
and live numbers (viewers, followers, subscribers). Everything except Twitch's own data
comes from Sylva (https://github.com/haruxee/sylva, API in its docs/api.md).

Sylva Desktop used to be called Cortex, and before that Pine. Some internal names keep an old
name on purpose: the `pine-core` crate, the `pine` Cargo package and the `pine.exe` it builds
(older copies update themselves with `cargo build -p pine`), the app identifier
`app.pine.desktop` (it is also the credential store service name and the app data folder, so
saved sign-ins and settings keep working), the GitHub repository, and the MSI upgrade code
(still the one Tauri made for "Cortex", so the `.msi` upgrades an old install in place). The
Windows installer installs into a new "Sylva Desktop" folder, so its hook
(`src-tauri/windows/installer-hooks.nsh`) runs the old Cortex uninstaller silently afterwards.

## Stack and why

| Layer | Choice | Reason |
| --- | --- | --- |
| App shell | Tauri 2 (Rust) | Small native binary that uses the system webview (WebView2 on Windows). All networking, secrets and state live in Rust. |
| UI | Plain HTML, CSS and JavaScript in `ui/` | No Node toolchain or bundler needed. The UI only renders and calls Rust commands. |
| HTTP | reqwest with the OS TLS stack | Uses the Windows and macOS certificate stores, so corporate proxies and antivirus TLS work. |
| Realtime | tokio-tungstenite | Sylva's live socket is a plain JSON websocket. |
| Secrets | keyring | Tokens go in Windows Credential Manager, macOS Keychain or Linux Secret Service. Never in files. |

egui or iced were considered. Tauri won because a feed of rich, variable length messages,
avatars and badges is far easier to lay out well in HTML, and the Rust core stays GUI free
and testable either way.

## Layout of the code

```
crates/pine-core/       Pure Rust library, no GUI code, unit tested
  src/activity.rs           The Activity model the window shows
  src/sylva.rs              Sylva: device code sign-in, history, alert controls,
                            overlay reload, replay, and the live socket
  src/twitch.rs             Twitch device code sign-in and Helix lists
  src/public.rs             Viewer count without a Twitch sign-in (DecAPI)
src-tauri/                  The desktop app
  src/commands.rs           Commands the UI calls with invoke()
  src/state.rs              Signed-in sessions, live task wiring
  src/secrets.rs            OS credential store
ui/                         index.html, styles.css, app.js
```

## Data sources

| Panel | Source | Endpoint |
| --- | --- | --- |
| Activity history | Sylva | `GET /v1/channels/{key}/history?filter=alerts&limit=100` |
| Live activity | Sylva | `GET /v1/channels/{key}/ws`, hello as `feed` with `lastSeq` |
| Mute, pause, skip alerts | Sylva | `POST /v1/channels/{key}/alerts/{mute,unmute,pause,resume,skip}` |
| Alert queue state | Sylva | `GET /v1/channels/{key}/state` |
| Reload overlays | Sylva | `POST /v1/channels/{key}/alerts/reload` |
| Replay an alert | Sylva | `POST /v1/channels/{key}/alerts/replay/{seq}` |
| Followers and subs totals | Twitch Helix | `total` of `GET /channels/followers` and `GET /subscriptions` |
| Viewers, live state | Twitch Helix | `GET /streams` (`viewer_count`), or DecAPI without Twitch |
| Chatters, subscriber and follower lists | Twitch Helix | as before |

## Authentication

* **Sylva:** device code flow (`POST /v1/device/code`, then poll `/v1/device/token`). The
  approval page opens with the code filled in. The API token is stored in the OS credential
  store and sent as `Authorization: Bearer`. A 401 means the streamer removed Sylva Desktop, so it
  asks to sign in again. A StreamElements token saved by older versions is deleted on start.
* **Twitch:** device code flow with a Public client, so no client secret ships in the app.
  The user registers a free app at dev.twitch.tv and pastes its Client ID once (or it can be
  baked in at build time with `CORTEX_TWITCH_CLIENT_ID`). Access tokens refresh
  automatically on a 401 and the new tokens are saved back to the credential store.

## Realtime flow

1. Connect to Sylva's socket with the API token and say hello as `feed` with the last event
   seen, so a reconnect gets what it missed.
2. Each `event` frame (chat left out) becomes an `Activity` and goes to the window.
3. A ping every 10 seconds; a socket quiet for 25 seconds is dropped and opened again.
4. Drops retry with backoff from 0.5 to 10 seconds. A 401 stops and asks to sign in again.
