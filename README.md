# Cuepoint

A desktop control room for StreamElements streamers, written in Rust.

* **Activity feed:** follows, subs, gifted subs, bits, tips, channel point and store
  redemptions, raids. Live updates, filters, one click alert replay.
* **Quick actions:** mute or unmute alerts, pause or resume the alert queue, skip the current
  alert, reload overlays, and a stream timer (count up, or count down from N minutes).
* **Stats and audience:** live viewers, chatters, followers and subs at the top, plus
  searchable lists of current subscribers, followers and chatters.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it is built and why.

## Run it

1. Install Rust from https://rustup.rs and the Tauri prerequisites for your OS from
   https://v2.tauri.app/start/prerequisites/ (on Windows: Microsoft C++ Build Tools and
   WebView2, which Windows 10 and 11 already include).
2. Start the app:

   ```
   cargo run -p cuepoint
   ```

3. To build an installer, install the Tauri CLI once and build:

   ```
   cargo install tauri-cli --version "^2" --locked
   cd src-tauri
   cargo tauri build
   ```

## Sign in

**StreamElements (required).** Open https://streamelements.com/dashboard/account/channels,
click **Show secrets**, copy the **JWT token** and paste it into Cuepoint. It is stored in your
system's credential store (Windows Credential Manager, macOS Keychain, or Secret Service on
Linux). Treat it like a password.

**Twitch (optional, for viewers, chatters and the sub and follower lists).**

1. Go to https://dev.twitch.tv/console/apps and register an application.
2. Set **Client Type** to **Public**. Any OAuth Redirect URL works, for example `http://localhost`.
3. Copy the **Client ID** into the Audience panel and click **Connect Twitch**.
4. Enter the code Cuepoint shows on the Twitch page that opens, and approve.

To skip step 3 for everyone who uses your build, set `CUEPOINT_TWITCH_CLIENT_ID` when building.

## Develop

```
cargo test --workspace      # 14 tests: REST shapes, websocket protocol, Twitch auth
cargo clippy --workspace --all-targets
```

The UI is static files in `ui/`, embedded at build time. Edit and re-run.

## Known limits

* The mute and pause buttons show the state set from Cuepoint. If you mute alerts somewhere
  else, the button will not know until you press it.
* History loads the last 30 days, up to 100 items. New events stream in live after that.
* There is no "play next" control yet, because no supported StreamElements endpoint for it
  was found.
