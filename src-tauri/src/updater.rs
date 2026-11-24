//! In-app updates for Pine when it runs from a git checkout (the `update-and-run.bat` setup).
//!
//! The check runs `git fetch` and counts commits on the upstream branch that are not local yet.
//! Applying an update starts `update-and-run.bat --wait` in its own window and closes Pine;
//! the script waits for Pine to exit (Windows locks a running .exe), pulls, rebuilds and
//! starts the new version.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tauri::AppHandle;

/// Where this binary was built from. For a source checkout this is `<repo>/src-tauri`.
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn repo_root() -> Option<PathBuf> {
    let root = Path::new(MANIFEST_DIR).parent()?.to_path_buf();
    (root.join(".git").exists() && root.join("update-and-run.bat").exists()).then_some(root)
}

fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(root).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd
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
    /// False when Pine is not running from a git checkout; the UI hides the button.
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

/// Start the updater script in its own window, then close Pine so it can be rebuilt.
#[tauri::command]
pub fn apply_update(app: AppHandle) -> Result<(), String> {
    let root = repo_root().ok_or("Updates only work when Pine runs from its git folder.")?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = root.join("update-and-run.bat");
        // `start` opens a new console window for the script; the empty-titled host stays hidden.
        Command::new("cmd")
            .current_dir(&root)
            .raw_arg(format!("/C start \"Pine update\" \"{}\" --wait", script.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("could not start the updater: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .current_dir(&root)
            .arg("-c")
            .arg("sleep 2; git pull --ff-only && exec cargo run -p pine")
            .spawn()
            .map_err(|e| format!("could not start the updater: {e}"))?;
    }

    app.exit(0);
    Ok(())
}
