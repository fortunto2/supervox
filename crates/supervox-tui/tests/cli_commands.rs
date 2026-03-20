use chrono::Utc;
use supervox_agent::storage;
use supervox_agent::types::{
    ActionItem, Call, CallAnalysis, CallInsights, Mood, MoodSummary, ThemeCount,
};

fn make_call(id: &str, transcript: &str) -> Call {
    Call {
        id: id.into(),
        created_at: Utc::now(),
        duration_secs: 90.0,
        participants: vec!["Alice".into(), "Bob".into()],
        language: Some("en".into()),
        transcript: transcript.into(),
        translation: None,
        tags: vec![],
        audio_path: None,
    }
}

#[test]
fn delete_with_force_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let call = make_call("cli-del-1", "Delete me via CLI");

    storage::save_call(&dir, &call).unwrap();
    assert!(storage::load_call(&dir, "cli-del-1").is_ok());

    storage::delete_call(&dir, "cli-del-1").unwrap();
    assert!(storage::load_call(&dir, "cli-del-1").is_err());
}

#[test]
fn export_produces_markdown() {
    let call = make_call("cli-exp-1", "Hello from CLI export test");
    let md = storage::export_call_markdown(&call, None);

    assert!(md.starts_with("# Call —"));
    assert!(md.contains("**Duration:** 1m 30s"));
    assert!(md.contains("**Participants:** Alice, Bob"));
    assert!(md.contains("## Transcript"));
    assert!(md.contains("Hello from CLI export test"));
}

#[test]
fn export_to_file() {
    let tmp = tempfile::tempdir().unwrap();
    let call = make_call("cli-exp-2", "File export test");
    let md = storage::export_call_markdown(&call, None);

    let path = tmp.path().join("export.md");
    std::fs::write(&path, &md).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("File export test"));
}

#[test]
fn search_returns_matching_results() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let c1 = make_call("cli-s-1", "We discussed the quarterly budget review");
    let c2 = make_call("cli-s-2", "Planning the team offsite event");
    storage::save_call(&dir, &c1).unwrap();
    storage::save_call(&dir, &c2).unwrap();

    let matches = supervox_agent::tools::search::search_calls_in_dir(&dir, "budget").unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].call_id, "cli-s-1");
    assert!(matches[0].snippet.contains("budget"));
}

#[test]
fn search_json_output_is_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let call = make_call("cli-s-3", "JSON output test content");
    storage::save_call(&dir, &call).unwrap();

    let matches = supervox_agent::tools::search::search_calls_in_dir(&dir, "JSON").unwrap();
    let json = serde_json::to_string_pretty(&matches).unwrap();
    // Verify it's valid JSON by parsing back
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn call_insights_serialization_roundtrip() {
    let insights = CallInsights {
        recurring_themes: vec![
            ThemeCount {
                theme: "budget".into(),
                count: 5,
            },
            ThemeCount {
                theme: "planning".into(),
                count: 3,
            },
        ],
        mood_summary: MoodSummary {
            positive: 4,
            neutral: 2,
            negative: 1,
            mixed: 0,
        },
        open_action_items: vec![ActionItem {
            description: "Review proposal".into(),
            assignee: Some("Alice".into()),
            deadline: Some("2026-04-01".into()),
        }],
        key_patterns: vec!["Recurring budget discussions".into()],
        total_calls: 7,
        period: "2026-03-01 to 2026-03-20".into(),
    };

    let json = serde_json::to_string_pretty(&insights).unwrap();
    let parsed: CallInsights = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.total_calls, 7);
    assert_eq!(parsed.recurring_themes.len(), 2);
    assert_eq!(parsed.recurring_themes[0].theme, "budget");
    assert_eq!(parsed.mood_summary.positive, 4);
    assert_eq!(parsed.open_action_items.len(), 1);
    assert_eq!(parsed.key_patterns.len(), 1);
    assert_eq!(parsed.period, "2026-03-01 to 2026-03-20");
}

#[test]
fn call_insights_json_schema_output() {
    // Verify CallInsights can produce valid JSON matching expected fields
    let insights = CallInsights {
        recurring_themes: vec![],
        mood_summary: MoodSummary {
            positive: 0,
            neutral: 0,
            negative: 0,
            mixed: 0,
        },
        open_action_items: vec![],
        key_patterns: vec![],
        total_calls: 0,
        period: "".into(),
    };
    let json = serde_json::to_string(&insights).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.get("recurring_themes").unwrap().is_array());
    assert!(val.get("mood_summary").unwrap().is_object());
    assert!(val.get("total_calls").unwrap().is_number());
}

#[test]
fn enriched_context_includes_analysis_data() {
    // Test that save_analysis + load_analysis roundtrip works for enrichment
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let call = make_call("enrich-1", "Discussion about Q2 goals");
    storage::save_call(&dir, &call).unwrap();

    let analysis = CallAnalysis {
        summary: "Discussed Q2 objectives".into(),
        action_items: vec![ActionItem {
            description: "Draft Q2 plan".into(),
            assignee: None,
            deadline: None,
        }],
        follow_up_draft: None,
        decisions: vec![],
        open_questions: vec![],
        mood: Mood::Positive,
        themes: vec!["planning".into(), "goals".into()],
    };
    storage::save_analysis(&dir, "enrich-1", &analysis).unwrap();

    // Verify analysis can be loaded and used for context enrichment
    let loaded = storage::load_analysis(&dir, "enrich-1").unwrap().unwrap();
    assert_eq!(loaded.summary, "Discussed Q2 objectives");
    assert_eq!(loaded.themes, vec!["planning", "goals"]);
    assert_eq!(loaded.action_items.len(), 1);
}

#[test]
fn insights_cli_help_shows_command() {
    // Integration test: verify the insights subcommand is registered
    let output = std::process::Command::new("cargo")
        .args(["run", "-p", "supervox-tui", "--", "--help"])
        .output()
        .expect("failed to run cargo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("insights"),
        "CLI help should list 'insights' command"
    );
}
