//! In-app updates for Sylva Control.
//!
//! Installed copies (from the installers on GitHub Releases) use cargo-packager's updater: it
//! reads `latest.json` from the newest release, checks the download against `UPDATE_PUBKEY`,
//! installs it and restarts. The key and the file format are the ones the Tauri builds used,
//! so Tauri copies update to this one too. The rest of this file covers copies that run
//! from a git checkout.
//!
//! The check runs `git fetch` and counts commits on the upstream branch that are not local yet.
//! Applying an update happens inside Sylva Control, with no console window: it pulls, rebuilds with
//! Cargo, then starts the new build and closes itself. Progress is sent to the window as
//! `Ev::UpdateStatus`.
//!
//! Windows does not let a running .exe be overwritten, but it does let it be renamed. So
//! before building, the running pine.exe is renamed to pine.old.exe; Cargo then writes the
//! new pine.exe, and the old file is deleted the next time Sylva Control starts.
//!
//! The Cargo package and the built program are still named `pine` (the app used to be called
//! Pine). Copies built before the rename update themselves with `cargo build -p pine` and then
//! start `pine.exe`, so renaming either would break their next update.

use crate::backend::{Ev, Notify, UpdateInfo};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

const UPDATE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDgwMjM3OURFNEE0MUM0MEMKUldRTXhFRkszbmtqZ09MZUdHS3V6VlRHd1Ivamhod0FpVUFUdVdya2FjNlFrUjRncFhtbWpLODUK";
const UPDATE_URL: &str = "https://github.com/SylvaBot/sylva-control/releases/latest/download/latest.json";

/// Where this binary was built from. For a source checkout this is `<repo>/app`.
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

static UPDATING: AtomicBool = AtomicBool::new(false);

fn repo_root() -> Option<PathBuf> {
    let root = Path::new(MANIFEST_DIR).parent()?.to_path_buf();
    root.join(".git").exists().then_some(root)
}

/// A command that never opens a console window on Windows.
fn quiet(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    let out = quiet("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git ({e}). Is Git installed?"))?;
    if out.status.success() {
        Ok(out)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!("git {} failed: {err}", args.join(" ")))
    }
}

fn check_blocking() -> Result<UpdateInfo, String> {
    let Some(root) = repo_root() else {
        return Ok(UpdateInfo { supported: false, behind: 0, changes: vec![] });
    };
    git(&root, &["fetch", "--quiet", "origin"])?;
    let count = git(&root, &["rev-list", "--count", "HEAD..@{u}"])?;
    let behind: u32 = String::from_utf8_lossy(&count.stdout).trim().parse().unwrap_or(0);
    let changes = if behind > 0 {
        let log = git(&root, &["log", "--format=%s", "-n", "10", "HEAD..@{u}"])?;
        String::from_utf8_lossy(&log.stdout)
            .lines()
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        vec![]
    };
    Ok(UpdateInfo { supported: true, behind, changes })
}

/// Is there a newer version? For a git checkout, new commits; for an installed copy, a newer release.
pub async fn check(notify: Notify, manual: bool) {
    let r = if repo_root().is_none() {
        tokio::task::spawn_blocking(check_release).await.map_err(|e| e.to_string()).and_then(|r| r)
    } else {
        tokio::task::spawn_blocking(check_blocking).await.map_err(|e| e.to_string()).and_then(|r| r)
    };
    notify(Ev::Update(r, manual));
}

fn check_release_update() -> Result<Option<cargo_packager_updater::Update>, String> {
    let config = cargo_packager_updater::Config {
        endpoints: vec![UPDATE_URL.parse().expect("valid update URL")],
        pubkey: UPDATE_PUBKEY.into(),
        windows: Some(cargo_packager_updater::WindowsConfig {
            install_mode: Some(cargo_packager_updater::WindowsUpdateInstallMode::Passive),
            ..Default::default()
        }),
    };
    let current: semver::Version = env!("CARGO_PKG_VERSION").parse().map_err(|e: semver::Error| e.to_string())?;
    cargo_packager_updater::check_update(current, config).map_err(|e| e.to_string())
}

/// Installed copy: is there a newer release than this one?
fn check_release() -> Result<UpdateInfo, String> {
    Ok(match check_release_update()? {
        Some(u) => {
            let mut changes = vec![format!("Sylva Control {}", u.version)];
            if let Some(notes) = u.body.as_deref() {
                changes.extend(notes.lines().map(str::trim).filter(|l| !l.is_empty()).take(9).map(str::to_string));
            }
            UpdateInfo { supported: true, behind: 1, changes }
        }
        None => UpdateInfo { supported: true, behind: 0, changes: vec![] },
    })
}

/// Installed copy: download the newest release, install it and restart.
fn run_release_update(notify: &Notify) -> Result<(), String> {
    emit(notify, "fetch", "Checking for the new version");
    let update = check_release_update()?.ok_or("Sylva Control is already up to date.")?;
    let version = update.version.clone();
    let got = std::cell::Cell::new(0u64);
    let shown: std::cell::Cell<Option<Option<u64>>> = std::cell::Cell::new(None);
    let bytes = update
        .download_extended(
            |chunk, total| {
                got.set(got.get() + chunk as u64);
                let pct = total.filter(|&t| t > 0).map(|t| got.get() * 100 / t);
                if shown.get() != Some(pct) {
                    shown.set(Some(pct));
                    let text = match pct {
                        Some(p) => format!("Downloading Sylva Control {version} ({p}%)"),
                        None => format!("Downloading Sylva Control {version}"),
                    };
                    emit(notify, "fetch", text);
                }
            },
            || {},
        )
        .map_err(|e| e.to_string())?;
    emit(notify, "restart", "Installing");
    update.install(bytes).map_err(|e| e.to_string())?;
    // On Windows the installer closes Sylva Control and opens the new one. Elsewhere, start it.
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

/// Progress for the window while an update runs: "download", "build", "restart" or "error"
/// (git checkouts); "fetch", "restart" or "error" (installed copies).
fn emit(notify: &Notify, step: &'static str, text: impl Into<String>) {
    notify(Ev::UpdateStatus { step, text: text.into() });
}

fn old_exe_path(exe: &Path) -> PathBuf {
    exe.with_file_name(if cfg!(windows) { "pine.old.exe" } else { "pine.old" })
}

/// Delete the previous build left behind by the last update. Called at startup.
pub fn clean_up_old_build() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe_path(&exe));
    }
}

fn run_update(notify: &Notify) -> Result<(), String> {
    let root = repo_root().ok_or("Updates only work when Sylva Control runs from its git folder.")?;

    emit(notify, "download", "Downloading the latest changes");
    git(&root, &["pull", "--ff-only"])?;

    let release = !cfg!(debug_assertions);
    let target = root.join("target").join(if release { "release" } else { "debug" });
    let exe_name = if cfg!(windows) { "pine.exe" } else { "pine" };
    let new_exe = target.join(exe_name);

    // Move the running program out of the way so Cargo can write the new one.
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let same = new_exe.exists()
        && std::fs::canonicalize(&current).ok() == std::fs::canonicalize(&new_exe).ok();
    let moved = if same {
        let old = old_exe_path(&new_exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&new_exe, &old).map_err(|e| format!("could not prepare the update: {e}"))?;
        Some(old)
    } else {
        None
    };
    let put_back = || {
        if let Some(old) = &moved {
            if !new_exe.exists() {
                let _ = std::fs::rename(old, &new_exe);
            }
        }
    };

    emit(notify, "build", "Building the new version");
    let mut args = vec!["build", "-p", "pine", "--color", "never"];
    if release {
        args.push("--release");
    }
    let mut child = match quiet("cargo")
        .current_dir(&root)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            put_back();
            return Err(format!("could not run cargo ({e}). Is Rust installed?"));
        }
    };

    // Cargo reports progress on stderr, one "Compiling crate vX" line per crate.
    let mut compiled = 0;
    let mut tail: Vec<String> = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("Compiling ") {
                compiled += 1;
                let name = rest.split_whitespace().next().unwrap_or(rest);
                emit(notify, "build", format!("Building {name} ({compiled} done)"));
            }
            tail.push(line);
            if tail.len() > 12 {
                tail.remove(0);
            }
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if !ok || !new_exe.exists() {
        put_back();
        return Err(format!("the build failed:\n{}", tail.join("\n")));
    }

    emit(notify, "restart", "Starting the new version");
    quiet(new_exe.to_string_lossy().as_ref())
        .current_dir(&root)
        .spawn()
        .map_err(|e| format!("the update is installed, but Sylva Control could not restart ({e}). Open it again."))?;
    Ok(())
}

/// Pull, rebuild and restart (git checkout), or download and install the release. Progress
/// arrives as `Ev::UpdateStatus`.
pub fn apply(notify: Notify) {
    if UPDATING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let r = if repo_root().is_none() { run_release_update(&notify) } else { run_update(&notify) };
        match r {
            Ok(()) => std::process::exit(0),
            Err(e) => emit(&notify, "error", e),
        }
        UPDATING.store(false, Ordering::SeqCst);
    });
}
