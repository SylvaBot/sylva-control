//! In-app updates for Cortex when it runs from a git checkout.
//!
//! The check runs `git fetch` and counts commits on the upstream branch that are not local yet.
//! Applying an update happens inside Cortex, with no console window: it pulls, rebuilds with
//! Cargo, then starts the new build and closes itself. Progress is sent to the window as
//! `update-status` events.
//!
//! Windows does not let a running .exe be overwritten, but it does let it be renamed. So
//! before building, the running pine.exe is renamed to pine.old.exe; Cargo then writes the
//! new pine.exe, and the old file is deleted the next time Cortex starts.
//!
//! The Cargo package and the built program are still named `pine` (the app used to be called
//! Pine). Copies built before the rename update themselves with `cargo build -p pine` and then
//! start `pine.exe`, so renaming either would break their next update.

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Where this binary was built from. For a source checkout this is `<repo>/src-tauri`.
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// False when Cortex is not running from a git checkout; the UI hides the button.
    supported: bool,
    /// Number of new commits available.
    behind: u32,
    /// Subjects of the new commits, newest first (at most 10).
    changes: Vec<String>,
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

#[tauri::command]
pub async fn check_for_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(check_blocking)
        .await
        .map_err(|e| e.to_string())?
}

/// Progress sent to the window while an update runs.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    /// "download", "build", "restart" or "error".
    step: &'static str,
    text: String,
    /// Number of crates compiled so far during the build step.
    compiled: u32,
}

fn emit(app: &AppHandle, step: &'static str, text: impl Into<String>, compiled: u32) {
    let _ = app.emit("update-status", Status { step, text: text.into(), compiled });
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

fn run_update(app: &AppHandle) -> Result<(), String> {
    let root = repo_root().ok_or("Updates only work when Cortex runs from its git folder.")?;

    emit(app, "download", "Downloading the latest changes", 0);
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

    emit(app, "build", "Building the new version", 0);
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
                emit(app, "build", format!("Building {name}"), compiled);
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

    emit(app, "restart", "Starting the new version", compiled);
    quiet(new_exe.to_string_lossy().as_ref())
        .current_dir(&root)
        .spawn()
        .map_err(|e| format!("the update is installed, but Cortex could not restart ({e}). Open it again."))?;
    Ok(())
}

/// Pull, rebuild and restart, all inside Cortex. Progress arrives as `update-status` events.
#[tauri::command]
pub fn apply_update(app: AppHandle) -> Result<(), String> {
    if UPDATING.swap(true, Ordering::SeqCst) {
        return Err("An update is already running.".into());
    }
    std::thread::spawn(move || {
        match run_update(&app) {
            Ok(()) => app.exit(0),
            Err(e) => emit(&app, "error", e, 0),
        }
        UPDATING.store(false, Ordering::SeqCst);
    });
    Ok(())
}
