//! Where supervox keeps its files: one home resolution (`directories::BaseDirs`) for everything.

use std::path::PathBuf;

/// The user's home directory (`.` when it cannot be resolved).
pub fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.supervox`
pub fn supervox_home() -> PathBuf {
    home_dir().join(".supervox")
}

/// `~/.supervox/voices`: `<Name>.wav` voiceprints.
pub fn voices_dir() -> PathBuf {
    supervox_home().join("voices")
}

/// `~/.supervox/live`: live transcript, sessions, speaking log.
pub fn live_dir() -> PathBuf {
    supervox_home().join("live")
}

/// `~/.supervox/models`
pub fn models_dir() -> PathBuf {
    supervox_home().join("models")
}
