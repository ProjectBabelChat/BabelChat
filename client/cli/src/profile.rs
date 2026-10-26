//! Saved sign-ins: one JSON file per profile, readable only by the user.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Context;
use babel_sdk::Token;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Profile {
    pub server: String,
    pub name: String,
    pub token: Token,
}

/// `$XDG_CONFIG_HOME/babelchat` or the platform's equivalent.
pub fn default_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("babelchat")
}

fn path(dir: &Path, profile: &str) -> PathBuf {
    dir.join(format!("{profile}.json"))
}

pub fn load(dir: &Path, profile: &str) -> anyhow::Result<Option<Profile>> {
    let path = path(dir, profile);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {}", path.display()))?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn save(dir: &Path, profile: &str, value: &Profile) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = path(dir, profile);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(path)
}

/// `$XDG_CACHE_HOME/babelchat/models` or the platform's equivalent: on-device
/// translation models, shared by every profile.
pub fn models_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("babelchat")
        .join("models")
}

/// What the profile's sessions keep between runs (messages, translations).
pub fn cache_path(dir: &Path, profile: &str) -> PathBuf {
    dir.join(format!("{profile}.cache.json"))
}

/// Forgets the sign-in and its cache.
pub fn remove(dir: &Path, profile: &str) -> anyhow::Result<bool> {
    let _ = fs::remove_file(cache_path(dir, profile));
    match fs::remove_file(path(dir, profile)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// The user's locale from the environment ("ja_JP.UTF-8" → "ja-JP").
pub fn system_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| {
            value
                .split(['.', '@'])
                .next()
                .unwrap_or_default()
                .replace('_', "-")
        })
        .find(|tag| !tag.is_empty() && tag != "C" && tag != "POSIX")
        .unwrap_or_else(|| "en-US".to_string())
}
