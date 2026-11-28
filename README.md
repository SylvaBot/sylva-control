# Sylva Control

<img src="assets/logo.svg" alt="Sylva Control logo" width="96" />

A desktop control room for Sylva streamers, written in Rust, with a native window (no webview). Sylva Control is
the desktop app in the Sylva family. It used to be called Sylva Desktop, before that Cortex, and before that Pine.

* **Live numbers:** current viewers in large type (no Twitch sign-in needed), with followers
  and subscribers below when Twitch is connected.
* **Activity feed:** Sylva's activity, the same list every Sylva page shows: follows, subs,
  gifted subs, bits, raids, redemptions, ad breaks, polls and more. Each kind is its own color
  and icon. Live updates, filters, one click alert replay. Test events leave after 30 seconds.
* **Quick actions:** mute or unmute alerts, pause or resume the alert queue, skip the current
  alert, reload overlays.
* **Ad breaks:** a bar at the top shows when Twitch's next ad break runs, turns amber with a
  countdown in the last five minutes, and red with the time left while ads are running. Needs
  Twitch connected; if you connected before this was added, click **Reconnect Twitch** once.
* **Song requests:** viewers ask for songs in chat with `!sr` and a song name, a Spotify link
  or a YouTube link (searched on Spotify by its title). Songs play in your own Spotify app
  through Spotify's official Web API, so Spotify Premium is needed and Spotify must be open.
  Connect Spotify once on Sylva's Song requests page. A small player on Home, above Recent
  activity, controls it: play, pause, skip, seek, volume, the queue (play next, remove) and a
  switch to take requests or not. Songs play one after another on their own, and Sylva's Song
  requests page shows and controls the same player. Nothing is sent to Spotify until you press
  play, and if you put on something else in Spotify the player steps back until you press play.
* **Stream info:** change your Twitch title and category (with category search). The fields
  follow what is live on Twitch, refreshed every 30 seconds. Needs Twitch connected once.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it is built and why.

## Install

Download the latest installer from the
[Releases page](https://github.com/SylvaBot/sylva-control/releases/latest):

* **Windows:** `Sylva-Control_<version>_x64-setup.exe`. Run it; no admin rights needed. Sylva Control then
  appears in the Start menu and can be pinned to the taskbar. The installer is not code signed
  yet, so Windows SmartScreen may say it is from an unknown publisher: click **More info**, then
  **Run anyway**.
* **macOS:** the `.dmg` (Apple Silicon `aarch64`, Intel `x64`). It is not notarized yet, so the
  first time, right click Sylva Control in Applications and choose **Open**.
* **Linux:** the `.AppImage` or `.deb`.

Installed copies update themselves: Sylva Control checks the latest GitHub Release at start and
every 15 minutes, and an **Update** button appears at the top when there is a new version. Click
it and Sylva Control downloads, installs and restarts. On Linux this works for the AppImage;
`.deb` installs update through a new download. Your sign-ins are kept.

**Coming from Sylva Desktop or Cortex?** It updates itself to Sylva Control like any other update.
0.6.0 is the bridge: it is signed with the old update key, which older copies trust, and it trusts
the new key that signs 0.6.1 and later. On Windows the update removes the old Sylva Desktop or
Cortex copy and its shortcuts, so pin Sylva Control to the taskbar again. Sign-ins and settings
carry over.

**0.8.0 is the native rewrite.** Earlier versions were built with Tauri and drew their window
in a webview. 0.8.0 draws it natively with egui and looks the same. Tauri copies update to it
like any other update, and sign-ins and the Twitch Client ID carry over. The Display settings
(ad countdown, quick actions, compact rows) and the Activity filter start from their defaults
once, since the old ones lived in the webview's storage.

## Release a new version

Bump `version` in the root `Cargo.toml`, then push a tag such as `v0.8.1`. The **Release**
workflow builds the installers for Windows, macOS and Linux with
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) and publishes them as a
GitHub Release. You can also start it by hand from the Actions tab.

The release also publishes `latest.json`, which installed copies read to update themselves.
Update files are signed: the SylvaBot organization secret `TAURI_SIGNING_PRIVATE_KEY` holds the
private key (no password), and its public key is `UPDATE_PUBKEY` in `app/src/updater.rs`. Keep the
private key safe: losing it means installed copies can no longer update themselves, and anyone
holding it could sign an update.

## Run from source

1. Install Rust from https://rustup.rs. On Linux also install
   `libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev
   libxi-dev libssl-dev libdbus-1-dev libasound2-dev pkg-config`.
2. Start the app (the Cargo package is still called `pine`):

   ```
   cargo run -p pine
   ```

   `SYLVA_CONTROL_DEMO=1 cargo run -p pine` opens it with sample data and no sign-in, for
   looking at the layout.

3. To build an installer: `cargo install cargo-packager --locked`, build with
   `cargo build --release -p pine`, copy `target/release/pine` to `target/release/Sylva Control`
   (`.exe` on Windows), then run `cargo packager -p pine --release`. Installers land in `dist/`.

## Updates

If you run Sylva Control from a git clone, it checks for new changes when it starts and every 15
minutes after that. When some are ready, a notice appears and an **Update** button shows in
the top bar. Click it and Sylva Control downloads the changes and rebuilds itself, showing its progress,
then restarts. No console window is involved. You can also use **Check for updates** in the
Settings menu.

`update-and-run.bat` does the same from a console window, which is handy for the first build.
After that you can open `target\debug\pine.exe` directly, or pin it to the taskbar. (The
program keeps the name `pine.exe` so that copies built before the rename can still update.)

## Sign in

**Sylva (required).** Click **Sign in with Sylva** and approve Sylva Control in the browser page that
opens (the code is filled in). The token is stored in your system's credential store (Windows
Credential Manager, macOS Keychain, or Secret Service on Linux). You can remove Sylva Control any time
under Connected apps in Sylva's settings.

**Twitch (optional, needed for title, category, ad breaks and follower totals).** Sylva Control shows
your viewer count without it: it looks up your channel name on [DecAPI](https://decapi.me), a
free public service. Connect it from the Settings menu or the Stream info section:

1. Go to https://dev.twitch.tv/console/apps and register an application.
2. Set **Client Type** to **Public**. Any OAuth Redirect URL works, for example `http://localhost`.
3. Click **Connect Twitch** in the Settings menu, paste the **Client ID** and continue.
4. Enter the code Sylva Control shows on the Twitch page that opens, and approve.

To skip step 3 for everyone who uses your build, set `CORTEX_TWITCH_CLIENT_ID` when building (the older
`PINE_TWITCH_CLIENT_ID` still works).

## Develop

```
cargo test --workspace      # Sylva event mapping, Twitch auth, viewer count
cargo clippy --workspace --all-targets
```

The window is drawn in `app/src/app.rs` with egui. Shared pieces of Sylva's look (fonts, icons,
buttons, fields, the title bar) live in `crates/sylva-look`.

## Known limits

* The mute and pause buttons read Sylva's state when Sylva Control opens. A change made elsewhere
  shows after the next restart or button press.
* History loads the newest 100 items. New events stream in live after that.
