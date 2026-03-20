//! Integration tests with real audio fixtures.
//!
//! Fixture: tests/fixtures/speech-en-30s.wav — 30s English speech, mono 16kHz PCM.
//! Source: YouTube (CC-licensed educational content).

use std::path::Path;

fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/speech-en-30s.wav")
}

#[test]
fn read_real_speech_wav() {
    let chunk = voxkit::types::read_wav_file(&fixture_path()).unwrap();
    assert_eq!(chunk.sample_rate, 16000);
    // 30 seconds at 16kHz = 480_000 samples
    assert!(
        chunk.len() >= 470_000,
        "Expected ~480k samples, got {}",
        chunk.len()
    );
    assert!(chunk.len() <= 490_000);
    // Duration should be ~30s
    assert!(chunk.duration_ms >= 29_000);
    assert!(chunk.duration_ms <= 31_000);
}

#[test]
fn real_audio_has_nonzero_energy() {
    let chunk = voxkit::types::read_wav_file(&fixture_path()).unwrap();
    let rms: f32 = (chunk.samples.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
    // Real speech should have meaningful energy (not silence)
    assert!(rms > 0.001, "Expected non-silent audio, RMS = {rms}");
}

#[test]
fn vad_detects_speech_in_real_audio() {
    use voxkit::vad::{VadConfig, VadProcessor};

    let chunk = voxkit::types::read_wav_file(&fixture_path()).unwrap();
    let mut vad = VadProcessor::new_rms(VadConfig::default(), chunk.sample_rate);

    let events = vad.feed(&chunk.samples);
    let speech_starts = events
        .iter()
        .filter(|e| matches!(e, voxkit::vad::VadEvent::SpeechStart))
        .count();
    assert!(
        speech_starts >= 1,
        "VAD should detect speech, got {speech_starts} starts"
    );
}

/// Whisper local transcription — only runs if model is downloaded.
#[cfg(feature = "whisper")]
#[test]
fn whisper_transcribes_real_speech() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let model_path = Path::new(&home).join(".supervox/models/ggml-base.bin");
    if !model_path.exists() {
        eprintln!(
            "Skipping whisper test: model not found at {}",
            model_path.display()
        );
        return;
    }

    let chunk = voxkit::types::read_wav_file(&fixture_path()).unwrap();
    let transcript =
        voxkit::whisper_stt::WhisperStt::transcribe_file(&model_path, &chunk, "en").unwrap();

    assert!(
        !transcript.text.trim().is_empty(),
        "Whisper should produce text"
    );
    assert!(
        transcript.text.len() >= 20,
        "Expected substantial text, got: '{}'",
        transcript.text
    );
    assert!(!transcript.segments.is_empty(), "Should have segments");

    eprintln!(
        "Whisper transcript ({} chars): {}",
        transcript.text.len(),
        &transcript.text[..100.min(transcript.text.len())]
    );
}

/// OpenAI batch STT — requires OPENAI_API_KEY.
#[tokio::test]
async fn openai_stt_transcribes_real_speech() {
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!("Skipping OpenAI STT test: OPENAI_API_KEY not set");
            return;
        }
    };

    let wav_bytes = std::fs::read(fixture_path()).unwrap();

    let stt = voxkit::openai_stt::OpenAiStt::new(&api_key).with_language("en");
    let transcript = stt
        .transcribe_file_bytes(&wav_bytes, "test.wav", "audio/wav")
        .await
        .unwrap();

    assert!(
        !transcript.text.trim().is_empty(),
        "OpenAI should produce text"
    );
    assert!(
        transcript.text.len() >= 50,
        "Expected substantial text from 30s, got: '{}'",
        transcript.text
    );

    eprintln!(
        "OpenAI transcript ({} chars): {}",
        transcript.text.len(),
        &transcript.text[..100.min(transcript.text.len())]
    );
}

/// Parakeet local transcription — only runs if model is available.
#[cfg(feature = "parakeet")]
#[test]
fn parakeet_transcribes_real_speech() {
    let model_dir = voxkit::parakeet_stt::default_model_dir();
    let alt_dir = {
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::PathBuf::from(home)
            .join("startups/active/life2film/video-analyzer/models/parakeet-tdt")
    };

    let dir = if voxkit::parakeet_stt::model_exists(&model_dir) {
        model_dir
    } else if voxkit::parakeet_stt::model_exists(&alt_dir) {
        alt_dir
    } else {
        eprintln!("Skipping parakeet test: model not found");
        return;
    };

    let chunk = voxkit::types::read_wav_file(&fixture_path()).unwrap();
    let transcript =
        voxkit::parakeet_stt::ParakeetStt::transcribe_file(&dir, &chunk, "en").unwrap();

    assert!(
        !transcript.text.trim().is_empty(),
        "Parakeet should produce text"
    );
    assert!(
        transcript.text.len() >= 50,
        "Expected substantial text from 30s, got: '{}'",
        transcript.text
    );
    assert!(
        !transcript.segments.is_empty(),
        "Should have word-level segments"
    );

    eprintln!(
        "Parakeet transcript ({} chars, {} segments): {}",
        transcript.text.len(),
        transcript.segments.len(),
        &transcript.text[..100.min(transcript.text.len())]
    );
}
