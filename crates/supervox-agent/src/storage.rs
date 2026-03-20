use crate::types::{Call, Config};
use std::path::{Path, PathBuf};

/// Default calls directory: ~/.supervox/calls/
pub fn default_calls_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".supervox").join("calls"))
        .unwrap_or_else(|| PathBuf::from(".supervox/calls"))
}

/// Save a call to JSON file. Filename: <date>-<id>.json
pub fn save_call(calls_dir: &Path, call: &Call) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(calls_dir)?;
    let date = call.created_at.format("%Y%m%d");
    let filename = format!("{}-{}.json", date, call.id);
    let path = calls_dir.join(filename);
    let json = serde_json::to_string_pretty(call)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Load a call by ID from the calls directory.
pub fn load_call(calls_dir: &Path, call_id: &str) -> Result<Call, Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(calls_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            if name.ends_with(call_id) {
                let json = std::fs::read_to_string(&path)?;
                let call: Call = serde_json::from_str(&json)?;
                return Ok(call);
            }
        }
    }
    Err(format!("Call not found: {call_id}").into())
}

/// List all saved calls, sorted by created_at descending.
pub fn list_calls(calls_dir: &Path) -> Result<Vec<Call>, Box<dyn std::error::Error>> {
    if !calls_dir.exists() {
        return Ok(Vec::new());
    }
    let mut calls = Vec::new();
    for entry in std::fs::read_dir(calls_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            match std::fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str::<Call>(&json) {
                    Ok(call) => calls.push(call),
                    Err(e) => tracing::warn!("Skipping malformed call {}: {e}", path.display()),
                },
                Err(e) => tracing::warn!("Failed to read {}: {e}", path.display()),
            }
        }
    }
    calls.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(calls)
}

/// Default config path: ~/.supervox/config.toml
pub fn default_config_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".supervox").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".supervox/config.toml"))
}

/// Load config from a TOML file. If missing, create default and return it.
pub fn load_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    } else {
        let cfg = Config::default();
        save_default_config(path, &cfg)?;
        Ok(cfg)
    }
}

/// Write config to a TOML file, creating parent directories if needed.
pub fn save_default_config(path: &Path, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_call(id: &str, transcript: &str) -> Call {
        Call {
            id: id.into(),
            created_at: Utc::now(),
            duration_secs: 60.0,
            participants: vec!["Alice".into()],
            language: Some("en".into()),
            transcript: transcript.into(),
            translation: None,
            tags: vec![],
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("abc123", "Hello world");

        save_call(&dir, &call).unwrap();
        let loaded = load_call(&dir, "abc123").unwrap();
        assert_eq!(loaded.id, "abc123");
        assert_eq!(loaded.transcript, "Hello world");
        assert_eq!(loaded.participants, vec!["Alice"]);
    }

    #[test]
    fn load_nonexistent_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        let result = load_call(tmp.path(), "nope");
        assert!(result.is_err());
    }

    #[test]
    fn list_calls_returns_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();

        let mut c1 = make_call("first", "First call");
        c1.created_at = chrono::DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
            .unwrap()
            .to_utc();

        let mut c2 = make_call("second", "Second call");
        c2.created_at = chrono::DateTime::parse_from_rfc3339("2026-03-15T10:00:00Z")
            .unwrap()
            .to_utc();

        save_call(&dir, &c1).unwrap();
        save_call(&dir, &c2).unwrap();

        let calls = list_calls(&dir).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "second"); // newest first
        assert_eq!(calls[1].id, "first");
    }

    #[test]
    fn list_calls_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = list_calls(tmp.path()).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn list_calls_nonexistent_dir() {
        let calls = list_calls(Path::new("/nonexistent/path")).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn config_roundtrip_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        let cfg = Config {
            my_language: "en".into(),
            stt_backend: "openai".into(),
            llm_model: "gpt-4o".into(),
            summary_lag_secs: 10,
            capture: "mic".into(),
        };
        save_default_config(&path, &cfg).unwrap();

        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.my_language, "en");
        assert_eq!(loaded.stt_backend, "openai");
        assert_eq!(loaded.llm_model, "gpt-4o");
        assert_eq!(loaded.summary_lag_secs, 10);
        assert_eq!(loaded.capture, "mic");
    }

    #[test]
    fn config_creates_default_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("config.toml");

        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.my_language, "ru"); // default
        assert_eq!(cfg.summary_lag_secs, 5);
        assert!(path.exists(), "default config file should be created");

        // Verify written file is valid TOML
        let content = std::fs::read_to_string(&path).unwrap();
        let reloaded: Config = toml::from_str(&content).unwrap();
        assert_eq!(reloaded.my_language, "ru");
    }

    #[test]
    fn config_partial_toml_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        std::fs::write(&path, "my_language = \"de\"\n").unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.my_language, "de");
        assert_eq!(cfg.stt_backend, "realtime"); // default
        assert_eq!(cfg.summary_lag_secs, 5); // default
    }

    #[test]
    fn save_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nested").join("calls");
        let call = make_call("test", "transcript");

        save_call(&dir, &call).unwrap();
        assert!(dir.exists());
        let loaded = load_call(&dir, "test").unwrap();
        assert_eq!(loaded.id, "test");
    }
}
