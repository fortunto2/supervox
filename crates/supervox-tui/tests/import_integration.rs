//! Integration tests for the import command with real audio.
//!
//! These tests exercise the full import pipeline: read WAV → transcribe → save call.
//! Tests requiring OPENAI_API_KEY skip gracefully if key is not set.

use std::path::Path;

fn fixture_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/speech-en-30s.wav")
        .to_string_lossy()
        .to_string()
}

/// Import with --no-analyze --json should produce valid Call JSON (requires OPENAI_API_KEY for STT).
#[test]
fn import_real_audio_produces_call_json() {
    if std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("Skipping: OPENAI_API_KEY not set");
        return;
    }

    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "-p",
            "supervox-tui",
            "--",
            "import",
            &fixture_path(),
            "--no-analyze",
            "--json",
            "--language",
            "en",
        ])
        .env(
            "SUPERVOX_DATA_DIR",
            std::env::temp_dir().join("supervox-test-import"),
        )
        .output()
        .expect("failed to run cargo");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Import should succeed. stderr: {stderr}"
    );

    // Should output valid JSON
    let call: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("Expected valid JSON, got error {e}. stdout: {stdout}"));

    // Verify Call structure
    assert!(call.get("id").is_some(), "Call should have id");
    assert!(
        call.get("transcript").is_some(),
        "Call should have transcript"
    );
    assert!(
        call.get("created_at").is_some(),
        "Call should have created_at"
    );

    let transcript = call["transcript"].as_str().unwrap_or("");
    assert!(
        transcript.len() >= 20,
        "30s of speech should produce substantial transcript, got: '{transcript}'"
    );

    eprintln!(
        "Import transcript: {}...",
        &transcript[..80.min(transcript.len())]
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("supervox-test-import"));
}

/// Import should correctly detect WAV format and duration.
#[test]
fn import_detects_wav_properties() {
    let chunk = voxkit::types::read_wav_file(Path::new(&fixture_path())).unwrap();
    assert_eq!(chunk.sample_rate, 16000);
    assert!(chunk.duration_ms >= 29_000);
    assert!(chunk.duration_ms <= 31_000);
}
