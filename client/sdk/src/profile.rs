//! Saved sign-ins: one JSON file per profile, readable only by the user.
//! Every BabelChat app on the machine (the terminal client, the desktop
//! app) shares them, so a profile signed in from one opens in the others.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::Token;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Names the file in an error, which io errors do not.
fn at(path: &Path) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

pub fn load(dir: &Path, profile: &str) -> io::Result<Option<Profile>> {
    let path = path(dir, profile);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        })?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(at(&path)(error)),
    }
}

pub fn save(dir: &Path, profile: &str, value: &Profile) -> io::Result<PathBuf> {
    fs::create_dir_all(dir).map_err(at(dir))?;
    let path = path(dir, profile);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(at(&path))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)
        .map_err(at(&path))?;
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

/// Where the profile's sessions keep downloaded files (a directory).
pub fn media_cache_path(dir: &Path, profile: &str) -> PathBuf {
    dir.join(format!("{profile}.media"))
}

/// Forgets the sign-in, its cache and its downloaded files.
pub fn remove(dir: &Path, profile: &str) -> io::Result<bool> {
    let _ = fs::remove_file(cache_path(dir, profile));
    let _ = fs::remove_dir_all(media_cache_path(dir, profile));
    let path = path(dir, profile);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(at(&path)(error)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_round_trip_privately() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile {
            server: "http://127.0.0.1:7000".into(),
            name: "alice".into(),
            token: Token {
                jwt: "jwt".into(),
                user_id: "u1".into(),
                name: "alice".into(),
                expires_at: 1,
            },
        };
        assert!(load(dir.path(), "alice").unwrap().is_none());
        let path = save(dir.path(), "alice", &profile).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(load(dir.path(), "alice").unwrap().unwrap().name, "alice");
        assert!(remove(dir.path(), "alice").unwrap());
        assert!(!remove(dir.path(), "alice").unwrap());
    }
}
