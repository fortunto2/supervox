use crate::types::{
    ActionState, Call, CallAnalysis, CallFilter, CallStats, Config, ThemeCount, TrackedAction,
};
use std::collections::HashMap;
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
        let fname = path.file_name().unwrap_or_default().to_string_lossy();
        if path.extension().is_some_and(|e| e == "json") && !fname.ends_with(".analysis.json") {
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

/// Filter a list of calls by tag and/or date range.
/// Tags use case-insensitive OR matching. Date comparison uses `created_at.date_naive()`.
pub fn filter_calls(calls: &[Call], filter: &CallFilter) -> Vec<Call> {
    calls
        .iter()
        .filter(|call| {
            // Tag filter: OR logic, case-insensitive
            if !filter.tags.is_empty() {
                let has_tag = filter.tags.iter().any(|ft| {
                    let ft_lower = ft.to_lowercase();
                    call.tags.iter().any(|ct| ct.to_lowercase() == ft_lower)
                });
                if !has_tag {
                    return false;
                }
            }
            // Date filters
            let call_date = call.created_at.date_naive();
            if filter.since.is_some_and(|since| call_date < since) {
                return false;
            }
            if filter.until.is_some_and(|until| call_date > until) {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

/// Collect unique tags across all calls, sorted by frequency descending.
pub fn collect_tags(calls: &[Call]) -> Vec<ThemeCount> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for call in calls {
        for tag in &call.tags {
            let key = tag.to_lowercase();
            *counts.entry(key).or_default() += 1;
        }
    }
    let mut result: Vec<ThemeCount> = counts
        .into_iter()
        .map(|(theme, count)| ThemeCount { theme, count })
        .collect();
    result.sort_by(|a, b| b.count.cmp(&a.count).then(a.theme.cmp(&b.theme)));
    result
}

/// Format a Call (and optional CallAnalysis) as a self-contained markdown string.
pub fn export_call_markdown(call: &Call, analysis: Option<&CallAnalysis>) -> String {
    let mut md = String::new();
    let date = call.created_at.format("%Y-%m-%d %H:%M UTC");
    let dur = call.duration_secs as u64;
    let mins = dur / 60;
    let secs = dur % 60;

    md.push_str(&format!("# Call — {date}\n\n"));
    md.push_str(&format!("**Duration:** {mins}m {secs}s\n"));
    if !call.participants.is_empty() {
        md.push_str(&format!(
            "**Participants:** {}\n",
            call.participants.join(", ")
        ));
    }
    if let Some(lang) = &call.language {
        md.push_str(&format!("**Language:** {lang}\n"));
    }
    if !call.tags.is_empty() {
        md.push_str(&format!("**Tags:** {}\n", call.tags.join(", ")));
    }
    if let Some(audio) = &call.audio_path
        && let Some(filename) = std::path::Path::new(audio).file_name()
    {
        md.push_str(&format!("**Audio:** {}\n", filename.to_string_lossy()));
    }

    md.push_str("\n## Transcript\n\n");
    md.push_str(&call.transcript);
    md.push('\n');

    if let Some(tr) = &call.translation {
        md.push_str("\n## Translation\n\n");
        md.push_str(tr);
        md.push('\n');
    }

    if let Some(a) = analysis {
        md.push_str("\n## Summary\n\n");
        md.push_str(&a.summary);
        md.push('\n');

        if !a.action_items.is_empty() {
            md.push_str("\n## Action Items\n\n");
            for item in &a.action_items {
                md.push_str(&format!("- {}", item.description));
                if let Some(who) = &item.assignee {
                    md.push_str(&format!(" (@{who})"));
                }
                if let Some(when) = &item.deadline {
                    md.push_str(&format!(" — due {when}"));
                }
                md.push('\n');
            }
        }

        if !a.decisions.is_empty() {
            md.push_str("\n## Decisions\n\n");
            for d in &a.decisions {
                md.push_str(&format!("- {d}\n"));
            }
        }

        if !a.open_questions.is_empty() {
            md.push_str("\n## Open Questions\n\n");
            for q in &a.open_questions {
                md.push_str(&format!("- {q}\n"));
            }
        }

        md.push_str(&format!("\n**Mood:** {:?}\n", a.mood));

        if !a.themes.is_empty() {
            md.push_str(&format!("**Themes:** {}\n", a.themes.join(", ")));
        }
    }

    md
}

/// Return the expected WAV path for a call: `{date}-{id}.wav` inside calls_dir.
pub fn audio_path_for_call(calls_dir: &Path, call: &Call) -> PathBuf {
    let date = call.created_at.format("%Y%m%d");
    let filename = format!("{}-{}.wav", date, call.id);
    calls_dir.join(filename)
}

/// Check if a WAV audio file exists for the given call.
pub fn has_audio(calls_dir: &Path, call: &Call) -> bool {
    audio_path_for_call(calls_dir, call).exists()
}

/// Save analysis results as `{date}-{id}.analysis.json` alongside the call file.
pub fn save_analysis(
    calls_dir: &Path,
    call_id: &str,
    analysis: &CallAnalysis,
) -> Result<(), Box<dyn std::error::Error>> {
    // Find call file to derive the base name
    for entry in std::fs::read_dir(calls_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            if name.ends_with(call_id) && !name.contains(".analysis") {
                let analysis_filename = format!("{name}.analysis.json");
                let analysis_path = calls_dir.join(analysis_filename);
                let json = serde_json::to_string_pretty(analysis)?;
                std::fs::write(analysis_path, json)?;
                return Ok(());
            }
        }
    }
    Err(format!("Call not found for analysis: {call_id}").into())
}

/// Load persisted analysis for a call. Returns None if no analysis file exists.
pub fn load_analysis(
    calls_dir: &Path,
    call_id: &str,
) -> Result<Option<CallAnalysis>, Box<dyn std::error::Error>> {
    if !calls_dir.exists() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(calls_dir)? {
        let entry = entry?;
        let path = entry.path();
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        if filename.ends_with(".analysis.json") && filename.contains(call_id) {
            let json = std::fs::read_to_string(&path)?;
            let analysis: CallAnalysis = serde_json::from_str(&json)?;
            return Ok(Some(analysis));
        }
    }
    Ok(None)
}

/// Update call tags from analysis themes. Idempotent — no-op if tags match.
pub fn update_call_tags(
    calls_dir: &Path,
    call_id: &str,
    themes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut call = load_call(calls_dir, call_id)?;
    if call.tags == themes {
        return Ok(());
    }
    call.tags = themes.to_vec();
    save_call(calls_dir, &call)?;
    Ok(())
}

/// Delete a call by ID from the calls directory.
/// Finds the file by ID suffix match (same pattern as `load_call`).
pub fn delete_call(calls_dir: &Path, call_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(calls_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            if name.ends_with(call_id) && !name.contains(".analysis") {
                // Remove the call JSON
                std::fs::remove_file(&path)?;
                // Remove associated WAV if it exists
                let wav_path = path.with_extension("wav");
                if wav_path.exists() {
                    std::fs::remove_file(&wav_path)?;
                }
                // Remove associated analysis JSON if it exists
                let analysis_path = calls_dir.join(format!("{name}.analysis.json"));
                if analysis_path.exists() {
                    std::fs::remove_file(&analysis_path)?;
                }
                return Ok(());
            }
        }
    }
    Err(format!("Call not found: {call_id}").into())
}

/// Compute aggregate statistics across all saved calls.
pub fn compute_stats(calls_dir: &Path) -> Result<CallStats, Box<dyn std::error::Error>> {
    let calls = list_calls(calls_dir)?;
    let now = chrono::Utc::now();
    let week_ago = now - chrono::Duration::days(7);
    let month_ago = now - chrono::Duration::days(30);

    let total_calls = calls.len();
    let total_duration_secs: f64 = calls.iter().map(|c| c.duration_secs).sum::<f64>().max(0.0);
    let calls_this_week = calls.iter().filter(|c| c.created_at >= week_ago).count();
    let calls_this_month = calls.iter().filter(|c| c.created_at >= month_ago).count();

    let mut analyzed_count = 0usize;
    let mut theme_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for call in &calls {
        if let Ok(Some(analysis)) = load_analysis(calls_dir, &call.id) {
            analyzed_count += 1;
            for theme in &analysis.themes {
                *theme_counts.entry(theme.clone()).or_default() += 1;
            }
        }
    }

    let mut top_themes: Vec<ThemeCount> = theme_counts
        .into_iter()
        .map(|(theme, count)| ThemeCount { theme, count })
        .collect();
    top_themes.sort_by(|a, b| b.count.cmp(&a.count));
    top_themes.truncate(5);

    Ok(CallStats {
        total_calls,
        total_duration_secs,
        analyzed_count,
        unanalyzed_count: total_calls - analyzed_count,
        top_themes,
        calls_this_week,
        calls_this_month,
    })
}

/// Default actions store path: ~/.supervox/actions.json
pub fn default_actions_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".supervox").join("actions.json"))
        .unwrap_or_else(|| PathBuf::from(".supervox/actions.json"))
}

/// Load the action completion store from JSON file.
/// Returns empty HashMap if file doesn't exist.
pub fn load_action_store(
    path: &Path,
) -> Result<HashMap<String, ActionState>, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let json = std::fs::read_to_string(path)?;
    let store: HashMap<String, ActionState> = serde_json::from_str(&json)?;
    Ok(store)
}

/// Save the action completion store to JSON file.
pub fn save_action_store(
    path: &Path,
    store: &HashMap<String, ActionState>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(store)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Mark an action as completed. Creates the entry if it doesn't exist.
pub fn set_action_completed(
    path: &Path,
    action_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = load_action_store(path)?;
    store.insert(
        action_id.to_string(),
        ActionState {
            completed: true,
            completed_at: Some(chrono::Utc::now()),
        },
    );
    save_action_store(path, &store)
}

/// Mark an action as incomplete (undo completion). Removes the entry.
pub fn set_action_incomplete(
    path: &Path,
    action_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = load_action_store(path)?;
    store.remove(action_id);
    save_action_store(path, &store)
}

/// Scan all calls + analyses, generate TrackedAction list enriched with completion state.
/// Optionally filters by CallFilter and include_completed flag.
pub fn list_tracked_actions(
    calls_dir: &Path,
    actions_path: &Path,
    filter: &CallFilter,
    include_completed: bool,
) -> Result<Vec<TrackedAction>, Box<dyn std::error::Error>> {
    let all_calls = list_calls(calls_dir)?;
    let calls = filter_calls(&all_calls, filter);
    let store = load_action_store(actions_path)?;

    let mut actions = Vec::new();
    for call in &calls {
        if let Ok(Some(analysis)) = load_analysis(calls_dir, &call.id) {
            for item in &analysis.action_items {
                let aid = crate::types::action_id(&call.id, &item.description);
                let state = store.get(&aid).cloned().unwrap_or(ActionState {
                    completed: false,
                    completed_at: None,
                });
                if !include_completed && state.completed {
                    continue;
                }
                actions.push(TrackedAction {
                    action_id: aid,
                    call_id: call.id.clone(),
                    call_date: call.created_at,
                    description: item.description.clone(),
                    assignee: item.assignee.clone(),
                    deadline: item.deadline.clone(),
                    state,
                });
            }
        }
    }

    // Sort by call date descending (newest first)
    actions.sort_by(|a, b| b.call_date.cmp(&a.call_date));
    Ok(actions)
}

/// Find a tracked action by ID prefix. Returns (action_id, description) if found.
pub fn find_action_by_prefix(
    calls_dir: &Path,
    actions_path: &Path,
    prefix: &str,
) -> Result<Option<TrackedAction>, Box<dyn std::error::Error>> {
    let actions = list_tracked_actions(
        calls_dir,
        actions_path,
        &CallFilter::default(),
        true, // search all including completed
    )?;
    let matches: Vec<_> = actions
        .into_iter()
        .filter(|a| a.action_id.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.into_iter().next().unwrap())),
        _ => Err(format!(
            "Ambiguous prefix \"{prefix}\" — matches {} actions. Use more characters.",
            matches.len()
        )
        .into()),
    }
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
            audio_path: None,
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
            llm_backend: "auto".into(),
            ollama_model: "llama3.2:3b".into(),
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

    #[test]
    fn delete_call_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("del123", "To be deleted");

        save_call(&dir, &call).unwrap();
        assert!(load_call(&dir, "del123").is_ok());

        delete_call(&dir, "del123").unwrap();
        assert!(load_call(&dir, "del123").is_err());
    }

    #[test]
    fn delete_call_removes_wav_and_analysis() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("delaudio", "To be deleted with audio");
        save_call(&dir, &call).unwrap();

        // Create associated WAV file
        let wav_path = audio_path_for_call(&dir, &call);
        std::fs::write(&wav_path, b"RIFF fake wav").unwrap();
        assert!(wav_path.exists());

        // Create associated analysis file
        let analysis = make_analysis();
        save_analysis(&dir, "delaudio", &analysis).unwrap();

        delete_call(&dir, "delaudio").unwrap();
        assert!(load_call(&dir, "delaudio").is_err());
        assert!(!wav_path.exists(), "WAV file should be deleted");
        assert!(
            load_analysis(&dir, "delaudio").unwrap().is_none(),
            "Analysis should be deleted"
        );
    }

    #[test]
    fn delete_nonexistent_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        let result = delete_call(tmp.path(), "nope");
        assert!(result.is_err());
    }

    #[test]
    fn export_markdown_full() {
        use crate::types::{ActionItem, Mood};

        let mut call = make_call("exp1", "Hello, how are you?\nI'm fine, thanks.");
        call.duration_secs = 125.0;
        call.participants = vec!["Alice".into(), "Bob".into()];
        call.language = Some("en".into());
        call.translation = Some("Привет, как дела?\nХорошо, спасибо.".into());
        call.tags = vec!["meeting".into()];

        let analysis = CallAnalysis {
            summary: "Discussed greetings".into(),
            action_items: vec![ActionItem {
                description: "Send follow-up".into(),
                assignee: Some("Alice".into()),
                deadline: Some("2026-03-25".into()),
            }],
            follow_up_draft: None,
            decisions: vec!["Go with plan A".into()],
            open_questions: vec!["Budget?".into()],
            mood: Mood::Positive,
            themes: vec!["greetings".into(), "planning".into()],
        };

        let md = export_call_markdown(&call, Some(&analysis));
        assert!(md.contains("# Call —"));
        assert!(md.contains("**Duration:** 2m 5s"));
        assert!(md.contains("**Participants:** Alice, Bob"));
        assert!(md.contains("**Language:** en"));
        assert!(md.contains("**Tags:** meeting"));
        assert!(md.contains("## Transcript"));
        assert!(md.contains("Hello, how are you?"));
        assert!(md.contains("## Translation"));
        assert!(md.contains("Привет, как дела?"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("Discussed greetings"));
        assert!(md.contains("## Action Items"));
        assert!(md.contains("Send follow-up (@Alice) — due 2026-03-25"));
        assert!(md.contains("## Decisions"));
        assert!(md.contains("Go with plan A"));
        assert!(md.contains("## Open Questions"));
        assert!(md.contains("Budget?"));
        assert!(md.contains("**Mood:** Positive"));
        assert!(md.contains("**Themes:** greetings, planning"));
    }

    #[test]
    fn export_markdown_no_analysis() {
        let call = make_call("exp2", "Just a transcript");
        let md = export_call_markdown(&call, None);
        assert!(md.contains("# Call —"));
        assert!(md.contains("## Transcript"));
        assert!(md.contains("Just a transcript"));
        assert!(!md.contains("## Summary"));
        assert!(!md.contains("## Action Items"));
    }

    #[test]
    fn export_markdown_with_translation() {
        let mut call = make_call("exp3", "Original text");
        call.translation = Some("Translated text".into());
        let md = export_call_markdown(&call, None);
        assert!(md.contains("## Translation"));
        assert!(md.contains("Translated text"));
    }

    #[test]
    fn export_markdown_with_audio_path() {
        let mut call = make_call("exp4", "Audio call");
        call.audio_path = Some("/home/user/.supervox/calls/20260320-exp4.wav".into());
        let md = export_call_markdown(&call, None);
        assert!(md.contains("**Audio:** 20260320-exp4.wav"));
    }

    #[test]
    fn export_markdown_without_audio_path() {
        let call = make_call("exp5", "No audio");
        let md = export_call_markdown(&call, None);
        assert!(!md.contains("**Audio:**"));
    }

    fn make_analysis() -> CallAnalysis {
        use crate::types::{ActionItem, Mood};
        CallAnalysis {
            summary: "Test summary".into(),
            action_items: vec![ActionItem {
                description: "Follow up".into(),
                assignee: Some("Alice".into()),
                deadline: None,
            }],
            follow_up_draft: None,
            decisions: vec!["Decision A".into()],
            open_questions: vec![],
            mood: Mood::Positive,
            themes: vec!["planning".into(), "budget".into()],
        }
    }

    #[test]
    fn save_and_load_analysis_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("ana123", "Some transcript");
        save_call(&dir, &call).unwrap();

        let analysis = make_analysis();
        save_analysis(&dir, "ana123", &analysis).unwrap();

        let loaded = load_analysis(&dir, "ana123").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.summary, "Test summary");
        assert_eq!(loaded.themes, vec!["planning", "budget"]);
    }

    #[test]
    fn load_analysis_not_found_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("noana", "Transcript");
        save_call(&dir, &call).unwrap();

        let loaded = load_analysis(&dir, "noana").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn load_analysis_nonexistent_dir_returns_none() {
        let result = load_analysis(Path::new("/nonexistent/dir"), "x").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn save_analysis_no_call_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        let analysis = make_analysis();
        let result = save_analysis(tmp.path(), "ghost", &analysis);
        assert!(result.is_err());
    }

    #[test]
    fn update_call_tags_sets_themes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("tag1", "Transcript");
        save_call(&dir, &call).unwrap();

        let themes = vec!["meeting".to_string(), "budget".to_string()];
        update_call_tags(&dir, "tag1", &themes).unwrap();

        let updated = load_call(&dir, "tag1").unwrap();
        assert_eq!(updated.tags, vec!["meeting", "budget"]);
    }

    #[test]
    fn update_call_tags_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let mut call = make_call("tag2", "Transcript");
        call.tags = vec!["a".into(), "b".into()];
        save_call(&dir, &call).unwrap();

        // Same tags — should be a no-op
        update_call_tags(&dir, "tag2", &["a".into(), "b".into()]).unwrap();
        let loaded = load_call(&dir, "tag2").unwrap();
        assert_eq!(loaded.tags, vec!["a", "b"]);
    }

    #[test]
    fn compute_stats_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let stats = compute_stats(tmp.path()).unwrap();
        assert_eq!(stats.total_calls, 0);
        assert_eq!(stats.total_duration_secs, 0.0);
        assert_eq!(stats.analyzed_count, 0);
        assert_eq!(stats.unanalyzed_count, 0);
        assert!(stats.top_themes.is_empty());
        assert_eq!(stats.calls_this_week, 0);
        assert_eq!(stats.calls_this_month, 0);
    }

    #[test]
    fn compute_stats_mixed_analyzed_unanalyzed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();

        let c1 = make_call("s1", "First call");
        let c2 = make_call("s2", "Second call");
        save_call(&dir, &c1).unwrap();
        save_call(&dir, &c2).unwrap();

        // Only analyze c1
        let analysis = make_analysis();
        save_analysis(&dir, "s1", &analysis).unwrap();

        let stats = compute_stats(&dir).unwrap();
        assert_eq!(stats.total_calls, 2);
        assert_eq!(stats.total_duration_secs, 120.0); // 60 + 60
        assert_eq!(stats.analyzed_count, 1);
        assert_eq!(stats.unanalyzed_count, 1);
        assert_eq!(stats.calls_this_week, 2);
        assert_eq!(stats.calls_this_month, 2);
    }

    #[test]
    fn compute_stats_theme_aggregation() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();

        let c1 = make_call("t1", "Call 1");
        let c2 = make_call("t2", "Call 2");
        save_call(&dir, &c1).unwrap();
        save_call(&dir, &c2).unwrap();

        let mut a1 = make_analysis();
        a1.themes = vec!["planning".into(), "budget".into()];
        save_analysis(&dir, "t1", &a1).unwrap();

        let mut a2 = make_analysis();
        a2.themes = vec!["planning".into(), "hiring".into()];
        save_analysis(&dir, "t2", &a2).unwrap();

        let stats = compute_stats(&dir).unwrap();
        assert_eq!(stats.analyzed_count, 2);
        // "planning" should appear with count 2 and be first
        assert_eq!(stats.top_themes[0].theme, "planning");
        assert_eq!(stats.top_themes[0].count, 2);
        assert_eq!(stats.top_themes.len(), 3); // planning, budget, hiring
    }

    #[test]
    fn compute_stats_old_calls_not_in_week() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();

        let mut old_call = make_call("old1", "Old call");
        old_call.created_at = chrono::DateTime::parse_from_rfc3339("2025-01-01T10:00:00Z")
            .unwrap()
            .to_utc();
        save_call(&dir, &old_call).unwrap();

        let stats = compute_stats(&dir).unwrap();
        assert_eq!(stats.total_calls, 1);
        assert_eq!(stats.calls_this_week, 0);
        assert_eq!(stats.calls_this_month, 0);
    }

    #[test]
    fn call_stats_serialization_roundtrip() {
        use crate::types::CallStats;
        let stats = CallStats {
            total_calls: 5,
            total_duration_secs: 300.0,
            analyzed_count: 3,
            unanalyzed_count: 2,
            top_themes: vec![crate::types::ThemeCount {
                theme: "planning".into(),
                count: 3,
            }],
            calls_this_week: 2,
            calls_this_month: 4,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let back: CallStats = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_calls, 5);
        assert_eq!(back.analyzed_count, 3);
        assert_eq!(back.top_themes[0].theme, "planning");
    }

    #[test]
    fn list_calls_excludes_analysis_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let call = make_call("lca1", "Transcript");
        save_call(&dir, &call).unwrap();

        let analysis = make_analysis();
        save_analysis(&dir, "lca1", &analysis).unwrap();

        let calls = list_calls(&dir).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "lca1");
    }

    // --- filter_calls tests ---

    fn make_tagged_call(id: &str, tags: &[&str], date: &str) -> Call {
        let dt = chrono::DateTime::parse_from_rfc3339(&format!("{date}T10:00:00Z"))
            .unwrap()
            .to_utc();
        Call {
            id: id.into(),
            created_at: dt,
            duration_secs: 60.0,
            participants: vec![],
            language: None,
            transcript: "transcript".into(),
            translation: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            audio_path: None,
        }
    }

    #[test]
    fn filter_calls_empty_filter_passthrough() {
        let calls = vec![
            make_tagged_call("a", &["meeting"], "2026-03-10"),
            make_tagged_call("b", &["budget"], "2026-03-15"),
        ];
        let filter = CallFilter::default();
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_calls_single_tag() {
        let calls = vec![
            make_tagged_call("a", &["meeting"], "2026-03-10"),
            make_tagged_call("b", &["budget"], "2026-03-15"),
        ];
        let filter = CallFilter {
            tags: vec!["meeting".into()],
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[test]
    fn filter_calls_multi_tag_or() {
        let calls = vec![
            make_tagged_call("a", &["meeting"], "2026-03-10"),
            make_tagged_call("b", &["budget"], "2026-03-15"),
            make_tagged_call("c", &["hiring"], "2026-03-20"),
        ];
        let filter = CallFilter {
            tags: vec!["meeting".into(), "budget".into()],
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_calls_tag_case_insensitive() {
        let calls = vec![make_tagged_call("a", &["Meeting"], "2026-03-10")];
        let filter = CallFilter {
            tags: vec!["MEETING".into()],
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filter_calls_since_only() {
        let calls = vec![
            make_tagged_call("a", &[], "2026-03-01"),
            make_tagged_call("b", &[], "2026-03-10"),
            make_tagged_call("c", &[], "2026-03-20"),
        ];
        let filter = CallFilter {
            since: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()),
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|c| c.id != "a"));
    }

    #[test]
    fn filter_calls_until_only() {
        let calls = vec![
            make_tagged_call("a", &[], "2026-03-01"),
            make_tagged_call("b", &[], "2026-03-10"),
            make_tagged_call("c", &[], "2026-03-20"),
        ];
        let filter = CallFilter {
            until: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()),
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|c| c.id != "c"));
    }

    #[test]
    fn filter_calls_combined_tag_and_date() {
        let calls = vec![
            make_tagged_call("a", &["meeting"], "2026-03-01"),
            make_tagged_call("b", &["meeting"], "2026-03-15"),
            make_tagged_call("c", &["budget"], "2026-03-15"),
        ];
        let filter = CallFilter {
            tags: vec!["meeting".into()],
            since: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()),
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "b");
    }

    #[test]
    fn filter_calls_no_matches() {
        let calls = vec![make_tagged_call("a", &["meeting"], "2026-03-10")];
        let filter = CallFilter {
            tags: vec!["nonexistent".into()],
            ..Default::default()
        };
        let result = filter_calls(&calls, &filter);
        assert!(result.is_empty());
    }

    // --- collect_tags tests ---

    #[test]
    fn collect_tags_empty() {
        let result = collect_tags(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn collect_tags_multiple_calls() {
        let calls = vec![
            make_tagged_call("a", &["meeting", "budget"], "2026-03-10"),
            make_tagged_call("b", &["meeting", "hiring"], "2026-03-15"),
            make_tagged_call("c", &["meeting"], "2026-03-20"),
        ];
        let result = collect_tags(&calls);
        assert_eq!(result[0].theme, "meeting");
        assert_eq!(result[0].count, 3);
        assert_eq!(result.len(), 3); // meeting, budget, hiring
    }

    #[test]
    fn collect_tags_sorted_by_frequency() {
        let calls = vec![
            make_tagged_call("a", &["rare"], "2026-03-10"),
            make_tagged_call("b", &["common", "rare"], "2026-03-15"),
            make_tagged_call("c", &["common"], "2026-03-20"),
        ];
        let result = collect_tags(&calls);
        assert_eq!(result[0].theme, "common");
        assert_eq!(result[0].count, 2);
        assert_eq!(result[1].theme, "rare");
        assert_eq!(result[1].count, 2);
    }

    // --- audio_path_for_call / has_audio tests ---

    #[test]
    fn audio_path_for_call_returns_expected_path() {
        let call = make_tagged_call("abc123", &[], "2026-03-20");
        let dir = Path::new("/tmp/calls");
        let path = audio_path_for_call(dir, &call);
        assert_eq!(path, PathBuf::from("/tmp/calls/20260320-abc123.wav"));
    }

    #[test]
    fn has_audio_false_when_no_wav() {
        let tmp = tempfile::tempdir().unwrap();
        let call = make_call("noaudio", "transcript");
        assert!(!has_audio(tmp.path(), &call));
    }

    #[test]
    fn has_audio_true_when_wav_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let call = make_call("withaudio", "transcript");
        let wav_path = audio_path_for_call(tmp.path(), &call);
        std::fs::write(&wav_path, b"RIFF fake wav").unwrap();
        assert!(has_audio(tmp.path(), &call));
    }

    // --- action store tests ---

    #[test]
    fn action_store_empty_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("actions.json");
        let store = load_action_store(&path).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn action_store_save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("actions.json");

        let mut store = HashMap::new();
        store.insert(
            "abc12345".to_string(),
            crate::types::ActionState {
                completed: true,
                completed_at: Some(chrono::Utc::now()),
            },
        );
        save_action_store(&path, &store).unwrap();

        let loaded = load_action_store(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded["abc12345"].completed);
    }

    #[test]
    fn set_action_completed_creates_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("actions.json");

        set_action_completed(&path, "test1234").unwrap();

        let store = load_action_store(&path).unwrap();
        assert!(store["test1234"].completed);
        assert!(store["test1234"].completed_at.is_some());
    }

    #[test]
    fn set_action_incomplete_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("actions.json");

        set_action_completed(&path, "test1234").unwrap();
        set_action_incomplete(&path, "test1234").unwrap();

        let store = load_action_store(&path).unwrap();
        assert!(!store.contains_key("test1234"));
    }

    #[test]
    fn list_tracked_actions_returns_actions_from_analyzed_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().join("calls");
        let actions_path = tmp.path().join("actions.json");

        let call = make_call("track1", "Some transcript");
        save_call(&calls_dir, &call).unwrap();

        let analysis = make_analysis(); // has 1 action item: "Follow up"
        save_analysis(&calls_dir, "track1", &analysis).unwrap();

        let actions =
            list_tracked_actions(&calls_dir, &actions_path, &CallFilter::default(), false).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].description, "Follow up");
        assert_eq!(actions[0].call_id, "track1");
        assert!(!actions[0].state.completed);
    }

    #[test]
    fn list_tracked_actions_excludes_completed_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().join("calls");
        let actions_path = tmp.path().join("actions.json");

        let call = make_call("track2", "Transcript");
        save_call(&calls_dir, &call).unwrap();
        let analysis = make_analysis();
        save_analysis(&calls_dir, "track2", &analysis).unwrap();

        // Mark the action as completed
        let aid = crate::types::action_id("track2", "Follow up");
        set_action_completed(&actions_path, &aid).unwrap();

        // Without include_completed: should be empty
        let actions =
            list_tracked_actions(&calls_dir, &actions_path, &CallFilter::default(), false).unwrap();
        assert!(actions.is_empty());

        // With include_completed: should show 1
        let actions =
            list_tracked_actions(&calls_dir, &actions_path, &CallFilter::default(), true).unwrap();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].state.completed);
    }

    #[test]
    fn list_tracked_actions_respects_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().join("calls");
        let actions_path = tmp.path().join("actions.json");

        let mut c1 = make_tagged_call("ft1", &["meeting"], "2026-03-10");
        c1.transcript = "Call 1".into();
        save_call(&calls_dir, &c1).unwrap();

        let mut c2 = make_tagged_call("ft2", &["budget"], "2026-03-15");
        c2.transcript = "Call 2".into();
        save_call(&calls_dir, &c2).unwrap();

        let a1 = make_analysis();
        save_analysis(&calls_dir, "ft1", &a1).unwrap();
        let a2 = make_analysis();
        save_analysis(&calls_dir, "ft2", &a2).unwrap();

        // Filter by tag "meeting" — should only include ft1's actions
        let filter = CallFilter {
            tags: vec!["meeting".into()],
            ..Default::default()
        };
        let actions = list_tracked_actions(&calls_dir, &actions_path, &filter, false).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].call_id, "ft1");
    }

    #[test]
    fn find_action_by_prefix_exact() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().join("calls");
        let actions_path = tmp.path().join("actions.json");

        let call = make_call("pfx1", "Transcript");
        save_call(&calls_dir, &call).unwrap();
        let analysis = make_analysis();
        save_analysis(&calls_dir, "pfx1", &analysis).unwrap();

        let actions =
            list_tracked_actions(&calls_dir, &actions_path, &CallFilter::default(), true).unwrap();
        let full_id = &actions[0].action_id;

        // Full ID match
        let found = find_action_by_prefix(&calls_dir, &actions_path, full_id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().action_id, *full_id);

        // Prefix match (first 4 chars)
        let found = find_action_by_prefix(&calls_dir, &actions_path, &full_id[..4]).unwrap();
        assert!(found.is_some());
    }

    #[test]
    fn find_action_by_prefix_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().join("calls");
        let actions_path = tmp.path().join("actions.json");
        std::fs::create_dir_all(&calls_dir).unwrap();

        let found = find_action_by_prefix(&calls_dir, &actions_path, "zzz").unwrap();
        assert!(found.is_none());
    }
}
