use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// A search match result when searching across past calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallMatch {
    pub call_id: String,
    pub snippet: String,
    pub score: f64,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            my_language: default_language(),
            stt_backend: default_stt_backend(),
            llm_model: default_llm_model(),
            summary_lag_secs: default_summary_lag(),
            capture: default_capture(),
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
    fn config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.my_language, "ru");
        assert_eq!(cfg.stt_backend, "realtime");
        assert_eq!(cfg.llm_model, "gemini-2.5-flash");
        assert_eq!(cfg.summary_lag_secs, 5);
        assert_eq!(cfg.capture, "mic+system");
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
    }
}
