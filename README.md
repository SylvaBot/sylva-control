# Cortex

<img src="assets/cortex-logo.svg" alt="Cortex logo" width="96" />

A desktop control room for StreamElements streamers, written in Rust. Cortex (Latin for bark)
is the desktop app in the Sylva family. It used to be called Pine.

* **Live numbers:** current viewers in large type (checked every 15 seconds, no Twitch sign-in
  needed), with followers, subscribers and what is new this stream below.
* **Activity feed:** follows, subs, gifted subs, bits, tips, channel point and store
  redemptions, raids. Each kind is its own color and icon. Live updates, filters, one click
  alert replay.
* **Quick actions:** mute or unmute alerts, pause or resume the alert queue, skip the current
  alert, reload overlays.
* **Ad breaks:** a bar at the top shows when Twitch's next ad break runs, turns amber with a
  countdown in the last five minutes, and red with the time left while ads are running. Needs
  Twitch connected; if you connected before this was added, click **Reconnect Twitch** once.
* **Stream info:** change your Twitch title and category (with category search). The fields
  follow what is live on Twitch, refreshed every 30 seconds. Needs Twitch connected once.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it is built and why.

## Install

Download the latest installer from the
[Releases page](https://github.com/haruxee/pine/releases/latest):

* **Windows:** `Cortex_<version>_x64-setup.exe`. Run it; no admin rights needed. Cortex then
  appears in the Start menu and can be pinned to the taskbar. The installer is not code signed
  yet, so Windows SmartScreen may say it is from an unknown publisher: click **More info**, then
  **Run anyway**. An `.msi` is there too if you prefer it.
* **macOS:** the `.dmg` (Apple Silicon `aarch64`, Intel `x64`). It is not notarized yet, so the
  first time, right click Cortex in Applications and choose **Open**.
* **Linux:** the `.AppImage`, `.deb` or `.rpm`.

From 0.2.0 on, installed copies update themselves: Cortex checks the latest GitHub Release at
start and every 15 minutes, and an **Update** button appears at the top when there is a new
version. Click it and Cortex downloads, installs and restarts. On Linux this works for the
AppImage; `.deb` and `.rpm` installs update through a new download. Your sign-ins are kept.

## Release a new version

Bump `version` in `Cargo.toml` and `src-tauri/tauri.conf.json`, then push a tag such as
`v0.3.0`. The **Release** workflow builds the installers for Windows, macOS and Linux and
publishes them as a GitHub Release. You can also start it by hand from the Actions tab.

The release also publishes `latest.json`, which installed copies read to update themselves.
Update files are signed: the repository secret `TAURI_SIGNING_PRIVATE_KEY` holds the private
key (no password), and its public key is `plugins.updater.pubkey` in `tauri.conf.json`. Keep
the private key safe: losing it means installed copies can no longer update themselves, and
anyone holding it could sign an update.

## Run from source

1. Install Rust from https://rustup.rs and the Tauri prerequisites for your OS from
   https://v2.tauri.app/start/prerequisites/ (on Windows: Microsoft C++ Build Tools and
   WebView2, which Windows 10 and 11 already include).
2. Start the app (the Cargo package is still called `pine`):

   ```
   cargo run -p pine
   ```

3. To build an installer, install the Tauri CLI once and build:

   ```
   cargo install tauri-cli --version "^2" --locked
   cd src-tauri
   cargo tauri build
   ```

   The installers land in `target/release/bundle/`.

## Updates

If you run Cortex from a git clone, it checks for new changes when it starts and every 15
minutes after that. When some are ready, a notice appears and an **Update** button shows in
the top bar. Click it and Cortex downloads the changes and rebuilds itself, showing its progress,
then restarts. No console window is involved. You can also use **Check for updates** in the
account menu.

`update-and-run.bat` does the same from a console window, which is handy for the first build.
After that you can open `target\debug\pine.exe` directly, or pin it to the taskbar. (The
program keeps the name `pine.exe` so that copies built before the rename can still update.)

## Sign in

**StreamElements (required).** Open https://streamelements.com/dashboard/account/channels,
click **Show secrets**, copy the **JWT token** and paste it into Cortex. It is stored in your
system's credential store (Windows Credential Manager, macOS Keychain, or Secret Service on
Linux). Treat it like a password.

**Twitch (optional, needed to change your title and category).** Cortex shows your viewer count
without it: it looks up your channel name from StreamElements on [DecAPI](https://decapi.me), a
free public service. StreamElements has no public way to change the title or category, so
that part goes through Twitch. Connect it from the account menu or the Stream info section:

1. Go to https://dev.twitch.tv/console/apps and register an application.
2. Set **Client Type** to **Public**. Any OAuth Redirect URL works, for example `http://localhost`.
3. Click **Connect Twitch** in the account menu, paste the **Client ID** and continue.
4. Enter the code Cortex shows on the Twitch page that opens, and approve.

To skip step 3 for everyone who uses your build, set `CORTEX_TWITCH_CLIENT_ID` when building (the older
`PINE_TWITCH_CLIENT_ID` still works).

## Develop

```
cargo test --workspace      # 18 tests: REST shapes, websocket protocol, Twitch auth, viewer count
cargo clippy --workspace --all-targets
```

The UI is static files in `ui/`, embedded at build time. Edit and re-run.

## Known limits

* The mute and pause buttons show the state set from Cortex. If you mute alerts somewhere
  else, the button will not know until you press it.
* History loads the last 30 days, up to 100 items. New events stream in live after that.
* There is no "play next" control yet, because no supported StreamElements endpoint for it
  was found.
