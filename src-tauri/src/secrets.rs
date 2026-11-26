//! Tokens live in the operating system's credential store (Windows Credential Manager,
//! macOS Keychain, Secret Service on Linux), never in plain files.

use pine_core::twitch::TwitchTokens;

// Kept from when the app was called Pine, so saved sign-ins survive the rename.
const SERVICE: &str = "app.pine.desktop";
/// Saved by Cortex versions that signed in to StreamElements; removed on start.
const SE_JWT: &str = "streamelements-jwt";
const TWITCH: &str = "twitch-tokens";
const SYLVA: &str = "sylva-token";

fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, name).map_err(|e| format!("credential store unavailable: {e}"))
}

fn read(name: &str) -> Result<Option<String>, String> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("could not read saved credentials: {e}")),
    }
}

fn write(name: &str, value: &str) -> Result<(), String> {
    entry(name)?
        .set_password(value)
        .map_err(|e| format!("could not save credentials: {e}"))
}

fn delete(name: &str) -> Result<(), String> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("could not remove saved credentials: {e}")),
    }
}

pub fn clear_se_jwt() -> Result<(), String> {
    delete(SE_JWT)
}

pub fn load_twitch() -> Result<Option<TwitchTokens>, String> {
    Ok(read(TWITCH)?.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn save_twitch(t: &TwitchTokens) -> Result<(), String> {
    let s = serde_json::to_string(t).map_err(|e| e.to_string())?;
    write(TWITCH, &s)
}

pub fn clear_twitch() -> Result<(), String> {
    delete(TWITCH)
}

pub fn load_sylva() -> Result<Option<String>, String> {
    read(SYLVA)
}

pub fn save_sylva(token: &str) -> Result<(), String> {
    write(SYLVA, token)
}

pub fn clear_sylva() -> Result<(), String> {
    delete(SYLVA)
}
