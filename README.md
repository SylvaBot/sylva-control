# Pine

A desktop control room for StreamElements streamers, written in Rust.

* **Live numbers:** current viewers in large type (checked every 15 seconds, no Twitch sign-in
  needed), with followers, subscribers and what is new this stream below.
* **Activity feed:** follows, subs, gifted subs, bits, tips, channel point and store
  redemptions, raids. Each kind is its own color and icon. Live updates, filters, one click
  alert replay.
* **Quick actions:** mute or unmute alerts, pause or resume the alert queue, skip the current
  alert, reload overlays.
* **Stream info:** change your Twitch title and category (with category search). The fields
  follow what is live on Twitch, refreshed every 30 seconds. Needs Twitch connected once.

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

If you run Pine from a git clone, it checks for new changes when it starts and every 15
minutes after that. When some are ready, a notice appears and an **Update** button shows in
the top bar. Click it and Pine downloads the changes and rebuilds itself, showing its progress,
then restarts. No console window is involved. You can also use **Check for updates** in the
account menu.

`update-and-run.bat` does the same from a console window, which is handy for the first build.
After that you can open `target\debug\pine.exe` directly, or pin it to the taskbar.

## Sign in

**StreamElements (required).** Open https://streamelements.com/dashboard/account/channels,
click **Show secrets**, copy the **JWT token** and paste it into Pine. It is stored in your
system's credential store (Windows Credential Manager, macOS Keychain, or Secret Service on
Linux). Treat it like a password.

**Twitch (optional, needed to change your title and category).** Pine shows your viewer count
without it: it looks up your channel name from StreamElements on [DecAPI](https://decapi.me), a
free public service. StreamElements has no public way to change the title or category, so
that part goes through Twitch. Connect it from the account menu or the Stream info section:

1. Go to https://dev.twitch.tv/console/apps and register an application.
2. Set **Client Type** to **Public**. Any OAuth Redirect URL works, for example `http://localhost`.
3. Click **Connect Twitch** in the account menu, paste the **Client ID** and continue.
4. Enter the code Pine shows on the Twitch page that opens, and approve.

To skip step 3 for everyone who uses your build, set `PINE_TWITCH_CLIENT_ID` when building.

## Develop

```
cargo test --workspace      # 18 tests: REST shapes, websocket protocol, Twitch auth, viewer count
cargo clippy --workspace --all-targets
```

The UI is static files in `ui/`, embedded at build time. Edit and re-run.

## Known limits

* The mute and pause buttons show the state set from Pine. If you mute alerts somewhere
  else, the button will not know until you press it.
* History loads the last 30 days, up to 100 items. New events stream in live after that.
* There is no "play next" control yet, because no supported StreamElements endpoint for it
  was found.
