# Pine

A desktop control room for StreamElements streamers, written in Rust.

* **Live numbers:** current viewers in large type (checked every 15 seconds, no Twitch sign-in
  needed), with followers, subscribers and what is new this stream below.
* **Activity feed:** follows, subs, gifted subs, bits, tips, channel point and store
  redemptions, raids. Each kind is its own color and icon. Live updates, filters, one click
  alert replay.
* **Quick actions menu:** mute or unmute alerts, pause or resume the alert queue, skip the
  current alert, reload overlays.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it is built and why.

## Run it

1. Install Rust from https://rustup.rs and the Tauri prerequisites for your OS from
   https://v2.tauri.app/start/prerequisites/ (on Windows: Microsoft C++ Build Tools and
   WebView2, which Windows 10 and 11 already include).
2. Start the app:

   ```
   cargo run -p pine
   ```

3. To build an installer, install the Tauri CLI once and build:

   ```
   cargo install tauri-cli --version "^2" --locked
   cd src-tauri
   cargo tauri build
   ```

## Updates

If you run Pine from a git clone, it checks for new changes every 15 minutes. When some are
ready, an **Update ready** button appears in the top bar. Click it and Pine closes, pulls the
changes, rebuilds and reopens. You can also use **Check for updates** in the account menu, or
double-click `update-and-run.bat`.

## Sign in

**StreamElements (required).** Open https://streamelements.com/dashboard/account/channels,
click **Show secrets**, copy the **JWT token** and paste it into Pine. It is stored in your
system's credential store (Windows Credential Manager, macOS Keychain, or Secret Service on
Linux). Treat it like a password.

**Twitch (optional).** Pine shows your viewer count without it: it looks up your channel name
from StreamElements on [DecAPI](https://decapi.me), a free public service. If you prefer to read
the count straight from the Twitch API, connect Twitch from the account menu:

1. Go to https://dev.twitch.tv/console/apps and register an application.
2. Set **Client Type** to **Public**. Any OAuth Redirect URL works, for example `http://localhost`.
3. Click **Connect Twitch** in the account menu, paste the **Client ID** and continue.
4. Enter the code Pine shows on the Twitch page that opens, and approve.

To skip step 3 for everyone who uses your build, set `PINE_TWITCH_CLIENT_ID` when building.

## Develop

```
cargo test --workspace      # 17 tests: REST shapes, websocket protocol, Twitch auth, viewer count
cargo clippy --workspace --all-targets
```

The UI is static files in `ui/`, embedded at build time. Edit and re-run.

## Known limits

* The mute and pause buttons show the state set from Pine. If you mute alerts somewhere
  else, the button will not know until you press it.
* History loads the last 30 days, up to 100 items. New events stream in live after that.
* There is no "play next" control yet, because no supported StreamElements endpoint for it
  was found.
