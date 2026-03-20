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

/// SuperVox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_language")]
    pub my_language: String,
    #[serde(default = "default_stt_backend")]
    pub stt_backend: String,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    #[serde(default = "default_summary_lag")]
    pub summary_lag_secs: u32,
    #[serde(default = "default_capture")]
    pub capture: String,
    #[serde(default = "default_llm_backend")]
    pub llm_backend: String,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
}

fn default_language() -> String {
    "ru".into()
}
fn default_stt_backend() -> String {
    "realtime".into()
}
fn default_llm_model() -> String {
    "gemini-2.5-flash".into()
}
fn default_summary_lag() -> u32 {
    5
}
fn default_capture() -> String {
    "mic+system".into()
}
fn default_llm_backend() -> String {
    "auto".into()
}
fn default_ollama_model() -> String {
    "llama3.2:3b".into()
}

impl Config {
    /// Returns the effective LLM model based on backend config and env override.
    pub fn effective_model(&self) -> &str {
        // --local flag sets this env var
        let backend =
            std::env::var("SUPERVOX_LLM_BACKEND").unwrap_or_else(|_| self.llm_backend.clone());
        if backend == "ollama" {
            &self.ollama_model
        } else {
            &self.llm_model
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            my_language: default_language(),
            stt_backend: default_stt_backend(),
            llm_model: default_llm_model(),
            summary_lag_secs: default_summary_lag(),
            capture: default_capture(),
            llm_backend: default_llm_backend(),
            ollama_model: default_ollama_model(),
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
        assert_eq!(cfg.stt_backend, "realtime");
        assert_eq!(cfg.llm_model, "gemini-2.5-flash");
        assert_eq!(cfg.summary_lag_secs, 5);
        assert_eq!(cfg.capture, "mic+system");
        assert_eq!(cfg.llm_backend, "auto");
        assert_eq!(cfg.ollama_model, "llama3.2:3b");
    }

    #[test]
    fn config_partial_json() {
        let json = r#"{"my_language": "en"}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.my_language, "en");
        assert_eq!(cfg.llm_model, "gemini-2.5-flash"); // default
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
}
