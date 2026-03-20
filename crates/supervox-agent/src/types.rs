use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A timestamped bookmark placed during a live call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Bookmark {
    pub timestamp_secs: f64,
    pub note: Option<String>,
}

/// A recorded call with transcript and metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Call {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub duration_secs: f64,
    #[serde(default)]
    pub participants: Vec<String>,
    pub language: Option<String>,
    pub transcript: String,
    pub translation: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<Bookmark>,
}

/// Post-call analysis output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallAnalysis {
    pub summary: String,
    pub action_items: Vec<ActionItem>,
    pub follow_up_draft: Option<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    pub mood: Mood,
    #[serde(default)]
    pub themes: Vec<String>,
}

/// An action item extracted from a call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionItem {
    pub description: String,
    pub assignee: Option<String>,
    pub deadline: Option<String>,
}

/// Overall mood/sentiment of a call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Positive,
    Neutral,
    Negative,
    Mixed,
}

/// Frequency count for a recurring theme across calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThemeCount {
    pub theme: String,
    pub count: usize,
}

/// Mood distribution across analyzed calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MoodSummary {
    pub positive: usize,
    pub neutral: usize,
    pub negative: usize,
    pub mixed: usize,
}

/// Cross-call insights aggregated from multiple call analyses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallInsights {
    pub recurring_themes: Vec<ThemeCount>,
    pub mood_summary: MoodSummary,
    pub open_action_items: Vec<ActionItem>,
    pub key_patterns: Vec<String>,
    pub total_calls: usize,
    pub period: String,
}

/// Aggregate call statistics.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallStats {
    pub total_calls: usize,
    pub total_duration_secs: f64,
    pub analyzed_count: usize,
    pub unanalyzed_count: usize,
    pub top_themes: Vec<ThemeCount>,
    pub calls_this_week: usize,
    pub calls_this_month: usize,
}

/// Filter criteria for narrowing down call lists.
#[derive(Debug, Clone, Default)]
pub struct CallFilter {
    pub tags: Vec<String>,
    pub since: Option<chrono::NaiveDate>,
    pub until: Option<chrono::NaiveDate>,
}

/// A search match result when searching across past calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallMatch {
    pub call_id: String,
    pub snippet: String,
    pub score: f64,
}

/// Generate a deterministic action item ID from call_id + description.
/// Returns first 8 chars of SHA-256 hex digest of "{call_id}:{description}".
pub fn action_id(call_id: &str, description: &str) -> String {
    let input = format!("{call_id}:{description}");
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)[..8].to_string()
}

/// Completion state of an action item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionState {
    pub completed: bool,
    pub completed_at: Option<DateTime<Utc>>,
}

/// A tracked action item enriched with ID, call context, and completion state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrackedAction {
    pub action_id: String,
    pub call_id: String,
    pub call_date: DateTime<Utc>,
    pub description: String,
    pub assignee: Option<String>,
    pub deadline: Option<String>,
    pub state: ActionState,
}

/// Speech-to-text backend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SttBackend {
    #[default]
    Realtime,
    Whisper,
    Parakeet,
}

impl std::fmt::Display for SttBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SttBackend::Realtime => write!(f, "realtime"),
            SttBackend::Whisper => write!(f, "whisper"),
            SttBackend::Parakeet => write!(f, "parakeet"),
        }
    }
}

/// Audio capture mode.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum CaptureMode {
    #[serde(rename = "mic")]
    Mic,
    #[serde(rename = "mic+system")]
    #[default]
    MicSystem,
}

impl CaptureMode {
    /// Whether system audio capture is included.
    pub fn includes_system(&self) -> bool {
        matches!(self, CaptureMode::MicSystem)
    }
}

/// LLM backend selection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackend {
    #[default]
    Auto,
    Ollama,
}

/// SuperVox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_language")]
    pub my_language: String,
    #[serde(default)]
    pub stt_backend: SttBackend,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    #[serde(default = "default_summary_lag")]
    pub summary_lag_secs: u32,
    #[serde(default)]
    pub capture: CaptureMode,
    #[serde(default)]
    pub llm_backend: LlmBackend,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default = "default_whisper_model")]
    pub whisper_model: String,
    #[serde(default = "default_ducking_threshold")]
    pub ducking_threshold: f32,
    #[serde(default = "default_true")]
    pub translate: bool,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "ru".into()
}
fn default_llm_model() -> String {
    "gemini-2.5-flash".into()
}
fn default_summary_lag() -> u32 {
    5
}
fn default_ollama_model() -> String {
    "llama3.2:3b".into()
}
fn default_whisper_model() -> String {
    "base".into()
}
fn default_ducking_threshold() -> f32 {
    0.05
}

impl Config {
    /// Returns the effective LLM model based on backend config and env override.
    pub fn effective_model(&self) -> &str {
        let backend_str = std::env::var("SUPERVOX_LLM_BACKEND").unwrap_or_default();
        let is_ollama = if backend_str.is_empty() {
            self.llm_backend == LlmBackend::Ollama
        } else {
            backend_str == "ollama"
        };
        if is_ollama {
            &self.ollama_model
        } else {
            &self.llm_model
        }
    }

    /// Validate config values, returning a list of warnings.
    /// Lenient: returns warnings but never rejects the config.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !(0.0..=1.0).contains(&self.ducking_threshold) {
            warnings.push(format!(
                "ducking_threshold {} is out of range 0.0–1.0, using as-is",
                self.ducking_threshold
            ));
        }
        if self.summary_lag_secs == 0 {
            warnings.push("summary_lag_secs is 0, summaries will fire on every chunk".into());
        }
        const KNOWN_WHISPER_MODELS: &[&str] = &["tiny", "base", "small", "medium"];
        if !KNOWN_WHISPER_MODELS.contains(&self.whisper_model.as_str()) {
            warnings.push(format!(
                "whisper_model \"{}\" is not a known model (tiny/base/small/medium)",
                self.whisper_model
            ));
        }
        warnings
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            my_language: default_language(),
            stt_backend: SttBackend::default(),
            llm_model: default_llm_model(),
            summary_lag_secs: default_summary_lag(),
            capture: CaptureMode::default(),
            llm_backend: LlmBackend::default(),
            ollama_model: default_ollama_model(),
            whisper_model: default_whisper_model(),
            ducking_threshold: default_ducking_threshold(),
            translate: default_true(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn call_serialization_roundtrip() {
        let call = Call {
            id: "test-123".into(),
            created_at: Utc::now(),
            duration_secs: 120.5,
            participants: vec!["Alice".into(), "Bob".into()],
            language: Some("en".into()),
            transcript: "Hello, how are you?".into(),
            translation: Some("Привет, как дела?".into()),
            tags: vec!["meeting".into()],
            audio_path: None,
            bookmarks: vec![],
        };
        let json = serde_json::to_string(&call).unwrap();
        let back: Call = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "test-123");
        assert_eq!(back.duration_secs, 120.5);
        assert_eq!(back.participants.len(), 2);
        assert_eq!(back.translation.as_deref(), Some("Привет, как дела?"));
    }

    #[test]
    fn call_analysis_serialization_roundtrip() {
        let analysis = CallAnalysis {
            summary: "Discussed project timeline".into(),
            action_items: vec![ActionItem {
                description: "Send proposal".into(),
                assignee: Some("Alice".into()),
                deadline: Some("2026-03-25".into()),
            }],
            follow_up_draft: Some("Dear team...".into()),
            decisions: vec!["Go with option A".into()],
            open_questions: vec!["Budget approval?".into()],
            mood: Mood::Positive,
            themes: vec!["planning".into(), "budget".into()],
        };
        let json = serde_json::to_string(&analysis).unwrap();
        let back: CallAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(back.summary, "Discussed project timeline");
        assert_eq!(back.action_items.len(), 1);
        assert_eq!(back.mood, Mood::Positive);
        assert_eq!(back.themes.len(), 2);
    }

    #[test]
    fn mood_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Mood::Positive).unwrap(),
            "\"positive\""
        );
        assert_eq!(
            serde_json::to_string(&Mood::Negative).unwrap(),
            "\"negative\""
        );
        assert_eq!(serde_json::to_string(&Mood::Mixed).unwrap(), "\"mixed\"");
    }

    #[test]
    fn mood_deserializes_lowercase() {
        let m: Mood = serde_json::from_str("\"neutral\"").unwrap();
        assert_eq!(m, Mood::Neutral);
    }

    #[test]
    fn call_match_roundtrip() {
        let m = CallMatch {
            call_id: "abc".into(),
            snippet: "...relevant text...".into(),
            score: 0.85,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: CallMatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_id, "abc");
        assert_eq!(back.score, 0.85);
    }

    #[test]
    fn action_id_deterministic() {
        let id1 = action_id("call-123", "Send proposal");
        let id2 = action_id("call-123", "Send proposal");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 8);
    }

    #[test]
    fn action_id_different_inputs() {
        let id1 = action_id("call-123", "Send proposal");
        let id2 = action_id("call-123", "Review budget");
        let id3 = action_id("call-456", "Send proposal");
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn action_id_is_hex() {
        let id = action_id("test", "test");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn action_state_serialization() {
        let state = ActionState {
            completed: true,
            completed_at: Some(Utc::now()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: ActionState = serde_json::from_str(&json).unwrap();
        assert!(back.completed);
        assert!(back.completed_at.is_some());
    }

    #[test]
    fn tracked_action_serialization() {
        let tracked = TrackedAction {
            action_id: "abcd1234".into(),
            call_id: "call-1".into(),
            call_date: Utc::now(),
            description: "Follow up".into(),
            assignee: Some("Alice".into()),
            deadline: Some("2026-03-25".into()),
            state: ActionState {
                completed: false,
                completed_at: None,
            },
        };
        let json = serde_json::to_string(&tracked).unwrap();
        let back: TrackedAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back.action_id, "abcd1234");
        assert!(!back.state.completed);
    }

    #[test]
    fn bookmark_serialization_roundtrip() {
        let bookmark = Bookmark {
            timestamp_secs: 42.5,
            note: Some("Important point".into()),
        };
        let json = serde_json::to_string(&bookmark).unwrap();
        let back: Bookmark = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timestamp_secs, 42.5);
        assert_eq!(back.note.as_deref(), Some("Important point"));
    }

    #[test]
    fn bookmark_without_note() {
        let bookmark = Bookmark {
            timestamp_secs: 10.0,
            note: None,
        };
        let json = serde_json::to_string(&bookmark).unwrap();
        let back: Bookmark = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timestamp_secs, 10.0);
        assert!(back.note.is_none());
    }

    #[test]
    fn call_with_bookmarks_roundtrip() {
        let call = Call {
            id: "bm-test".into(),
            created_at: Utc::now(),
            duration_secs: 300.0,
            participants: vec![],
            language: None,
            transcript: "test".into(),
            translation: None,
            tags: vec![],
            audio_path: None,
            bookmarks: vec![
                Bookmark {
                    timestamp_secs: 10.0,
                    note: None,
                },
                Bookmark {
                    timestamp_secs: 42.5,
                    note: Some("Key decision".into()),
                },
            ],
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("bookmarks"));
        let back: Call = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bookmarks.len(), 2);
        assert_eq!(back.bookmarks[1].timestamp_secs, 42.5);
    }

    #[test]
    fn call_without_bookmarks_backward_compat() {
        // Old JSON without bookmarks field must deserialize with empty vec
        let json = r#"{
            "id": "old-call",
            "created_at": "2025-12-01T10:00:00Z",
            "duration_secs": 300,
            "participants": ["Alice"],
            "language": "en",
            "transcript": "Old call without bookmarks",
            "translation": null,
            "tags": ["meeting"]
        }"#;
        let call: Call = serde_json::from_str(json).unwrap();
        assert_eq!(call.id, "old-call");
        assert!(call.bookmarks.is_empty());
    }

    #[test]
    fn call_without_bookmarks_omits_field() {
        let call = Call {
            id: "no-bm".into(),
            created_at: Utc::now(),
            duration_secs: 60.0,
            participants: vec![],
            language: None,
            transcript: "test".into(),
            translation: None,
            tags: vec![],
            audio_path: None,
            bookmarks: vec![],
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(!json.contains("bookmarks"));
    }

    #[test]
    fn config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.my_language, "ru");
        assert_eq!(cfg.stt_backend, SttBackend::Realtime);
        assert_eq!(cfg.llm_model, "gemini-2.5-flash");
        assert_eq!(cfg.summary_lag_secs, 5);
        assert_eq!(cfg.capture, CaptureMode::MicSystem);
        assert_eq!(cfg.llm_backend, LlmBackend::Auto);
        assert_eq!(cfg.ollama_model, "llama3.2:3b");
        assert_eq!(cfg.whisper_model, "base");
        assert_eq!(cfg.ducking_threshold, 0.05);
        assert!(cfg.translate);
    }

    #[test]
    fn config_translate_false() {
        let cfg: Config = toml::from_str("translate = false").unwrap();
        assert!(!cfg.translate);
    }

    #[test]
    fn config_partial_json() {
        let json = r#"{"my_language": "en"}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.my_language, "en");
        assert_eq!(cfg.llm_model, "gemini-2.5-flash"); // default
        assert_eq!(cfg.ducking_threshold, 0.05); // default
    }

    #[test]
    fn config_custom_ducking_threshold() {
        let json = r#"{"ducking_threshold": 0.1}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.ducking_threshold, 0.1);
    }

    #[test]
    fn call_optional_fields_default() {
        let json = r#"{
            "id": "x",
            "created_at": "2026-03-20T10:00:00Z",
            "duration_secs": 60,
            "transcript": "hello"
        }"#;
        let call: Call = serde_json::from_str(json).unwrap();
        assert!(call.participants.is_empty());
        assert!(call.language.is_none());
        assert!(call.translation.is_none());
        assert!(call.tags.is_empty());
        assert!(call.audio_path.is_none());
        assert!(call.bookmarks.is_empty());
    }

    #[test]
    fn call_without_audio_path_deserializes() {
        // Backward compat: existing JSON without audio_path must deserialize
        let json = r#"{
            "id": "old-call",
            "created_at": "2025-12-01T10:00:00Z",
            "duration_secs": 300,
            "participants": ["Alice"],
            "language": "en",
            "transcript": "Old call transcript",
            "translation": null,
            "tags": ["meeting"]
        }"#;
        let call: Call = serde_json::from_str(json).unwrap();
        assert_eq!(call.id, "old-call");
        assert!(call.audio_path.is_none());
    }

    #[test]
    fn call_with_audio_path_roundtrip() {
        let call = Call {
            id: "audio-test".into(),
            created_at: Utc::now(),
            duration_secs: 60.0,
            participants: vec![],
            language: None,
            transcript: "test".into(),
            translation: None,
            tags: vec![],
            audio_path: Some("/path/to/call.wav".into()),
            bookmarks: vec![],
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("audio_path"));
        let back: Call = serde_json::from_str(&json).unwrap();
        assert_eq!(back.audio_path.as_deref(), Some("/path/to/call.wav"));
    }

    #[test]
    fn call_without_audio_path_omits_field() {
        let call = Call {
            id: "no-audio".into(),
            created_at: Utc::now(),
            duration_secs: 60.0,
            participants: vec![],
            language: None,
            transcript: "test".into(),
            translation: None,
            tags: vec![],
            audio_path: None,
            bookmarks: vec![],
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(!json.contains("audio_path"));
    }

    #[test]
    fn stt_backend_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&SttBackend::Realtime).unwrap(),
            "\"realtime\""
        );
        assert_eq!(
            serde_json::to_string(&SttBackend::Whisper).unwrap(),
            "\"whisper\""
        );
        let rt: SttBackend = serde_json::from_str("\"realtime\"").unwrap();
        assert_eq!(rt, SttBackend::Realtime);
        let wh: SttBackend = serde_json::from_str("\"whisper\"").unwrap();
        assert_eq!(wh, SttBackend::Whisper);
        let pk: SttBackend = serde_json::from_str("\"parakeet\"").unwrap();
        assert_eq!(pk, SttBackend::Parakeet);
    }

    #[test]
    fn capture_mode_serde_roundtrip() {
        assert_eq!(serde_json::to_string(&CaptureMode::Mic).unwrap(), "\"mic\"");
        assert_eq!(
            serde_json::to_string(&CaptureMode::MicSystem).unwrap(),
            "\"mic+system\""
        );
        let mic: CaptureMode = serde_json::from_str("\"mic\"").unwrap();
        assert_eq!(mic, CaptureMode::Mic);
        let ms: CaptureMode = serde_json::from_str("\"mic+system\"").unwrap();
        assert_eq!(ms, CaptureMode::MicSystem);
    }

    #[test]
    fn llm_backend_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&LlmBackend::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&LlmBackend::Ollama).unwrap(),
            "\"ollama\""
        );
        let auto: LlmBackend = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(auto, LlmBackend::Auto);
        let oll: LlmBackend = serde_json::from_str("\"ollama\"").unwrap();
        assert_eq!(oll, LlmBackend::Ollama);
    }

    #[test]
    fn config_backward_compat_toml_strings() {
        // Existing TOML files with string values must parse correctly
        let toml_str = r#"
stt_backend = "realtime"
capture = "mic+system"
llm_backend = "auto"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.stt_backend, SttBackend::Realtime);
        assert_eq!(cfg.capture, CaptureMode::MicSystem);
        assert_eq!(cfg.llm_backend, LlmBackend::Auto);
    }

    #[test]
    fn config_toml_whisper_mic_ollama() {
        let toml_str = r#"
stt_backend = "whisper"
capture = "mic"
llm_backend = "ollama"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.stt_backend, SttBackend::Whisper);
        assert_eq!(cfg.capture, CaptureMode::Mic);
        assert_eq!(cfg.llm_backend, LlmBackend::Ollama);
    }

    #[test]
    fn capture_mode_includes_system() {
        assert!(CaptureMode::MicSystem.includes_system());
        assert!(!CaptureMode::Mic.includes_system());
    }

    #[test]
    fn stt_backend_display() {
        assert_eq!(SttBackend::Realtime.to_string(), "realtime");
        assert_eq!(SttBackend::Whisper.to_string(), "whisper");
        assert_eq!(SttBackend::Parakeet.to_string(), "parakeet");
    }

    #[test]
    fn validate_valid_config_no_warnings() {
        let cfg = Config::default();
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_ducking_threshold_out_of_range() {
        let cfg = Config {
            ducking_threshold: 1.5,
            ..Config::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ducking_threshold"));
    }

    #[test]
    fn validate_ducking_threshold_negative() {
        let cfg = Config {
            ducking_threshold: -0.1,
            ..Config::default()
        };
        assert!(!cfg.validate().is_empty());
    }

    #[test]
    fn validate_summary_lag_zero() {
        let cfg = Config {
            summary_lag_secs: 0,
            ..Config::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("summary_lag_secs"));
    }

    #[test]
    fn validate_unknown_whisper_model() {
        let cfg = Config {
            whisper_model: "xlarge".into(),
            ..Config::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("whisper_model"));
    }

    #[test]
    fn validate_known_whisper_models_ok() {
        for model in &["tiny", "base", "small", "medium"] {
            let cfg = Config {
                whisper_model: model.to_string(),
                ..Config::default()
            };
            assert!(cfg.validate().is_empty(), "model {model} should be valid");
        }
    }
}
