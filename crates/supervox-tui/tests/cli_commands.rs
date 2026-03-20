use chrono::Utc;
use supervox_agent::storage;
use supervox_agent::types::Call;

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
