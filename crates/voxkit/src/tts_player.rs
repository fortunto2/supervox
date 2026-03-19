//! Background TTS player — synthesizes and plays text with sentence splitting.
//!
//! Uses `OpenAiTts` from our `tts` module for synthesis and `rodio` for playback.
//! Supports stop/cancel mid-playback and sentence-level streaming.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use tokio::sync::mpsc;

use crate::tts::OpenAiTts;

/// Commands for the TTS player.
#[derive(Debug)]
pub enum TtsCommand {
    /// Speak the given text. Player will split into sentences and stream.
    Speak(String),
    /// Stop current playback immediately.
    Stop,
    /// Shut down the player task.
    Shutdown,
}

/// Errors from TTS player.
#[derive(Debug, Error)]
pub enum TtsPlayerError {
    /// TTS synthesis failed.
    #[error("TTS synthesis: {0}")]
    Synthesis(String),
    /// Audio playback failed.
    #[error("Playback: {0}")]
    Playback(String),
    /// Channel error.
    #[error("Channel: {0}")]
    Channel(String),
}

/// Background TTS player.
///
/// Runs as a Tokio task. Send `TtsCommand::Speak(text)` to synthesize and play.
/// Splits text into sentences for lower latency (starts playing first sentence
/// while synthesizing the rest).
pub struct TtsPlayer {
    cmd_tx: mpsc::Sender<TtsCommand>,
}

impl TtsPlayer {
    /// Start a background TTS player.
    ///
    /// Returns the player handle. The player task runs until `Shutdown` is sent
    /// or the handle is dropped.
    pub fn start(tts: OpenAiTts) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<TtsCommand>(16);
        tokio::spawn(player_task(tts, cmd_rx));
        Self { cmd_tx }
    }

    /// Queue text for speech synthesis and playback.
    pub async fn speak(&self, text: &str) -> Result<(), TtsPlayerError> {
        self.cmd_tx
            .send(TtsCommand::Speak(text.to_string()))
            .await
            .map_err(|e| TtsPlayerError::Channel(e.to_string()))
    }

    /// Stop current playback.
    pub async fn stop(&self) -> Result<(), TtsPlayerError> {
        self.cmd_tx
            .send(TtsCommand::Stop)
            .await
            .map_err(|e| TtsPlayerError::Channel(e.to_string()))
    }

    /// Shut down the player task.
    pub async fn shutdown(&self) -> Result<(), TtsPlayerError> {
        self.cmd_tx
            .send(TtsCommand::Shutdown)
            .await
            .map_err(|e| TtsPlayerError::Channel(e.to_string()))
    }

    /// Get the command sender (for manual control).
    pub fn sender(&self) -> mpsc::Sender<TtsCommand> {
        self.cmd_tx.clone()
    }
}

/// Main player task loop.
async fn player_task(tts: OpenAiTts, mut cmd_rx: mpsc::Receiver<TtsCommand>) {
    let stop_flag = Arc::new(AtomicBool::new(false));

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            TtsCommand::Speak(text) => {
                stop_flag.store(false, Ordering::SeqCst);

                // Split into sentences for streaming playback
                let sentences = split_sentences(&text);

                for sentence in &sentences {
                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }

                    let trimmed = sentence.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Synthesize
                    match tts.speak(trimmed).await {
                        Ok(mp3_bytes) => {
                            if stop_flag.load(Ordering::Relaxed) {
                                break;
                            }
                            // Play (blocking in a spawn_blocking)
                            let flag = stop_flag.clone();
                            let result = tokio::task::spawn_blocking(move || {
                                play_mp3_stoppable(&mp3_bytes, &flag)
                            })
                            .await;

                            if let Err(e) = result {
                                tracing::warn!("Playback task panicked: {e}");
                                break;
                            }
                            if let Ok(Err(e)) = result {
                                tracing::warn!("Playback error: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("TTS synthesis error: {e}");
                            break;
                        }
                    }
                }
            }
            TtsCommand::Stop => {
                stop_flag.store(true, Ordering::SeqCst);
            }
            TtsCommand::Shutdown => {
                stop_flag.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
}

/// Play MP3 bytes through the default audio output, with stop support.
///
/// Blocks until playback completes or `stop_flag` is set.
pub fn play_mp3_stoppable(
    mp3_bytes: &[u8],
    stop_flag: &Arc<AtomicBool>,
) -> Result<(), TtsPlayerError> {
    let cursor = Cursor::new(mp3_bytes.to_vec());

    let (_stream, stream_handle) =
        rodio::OutputStream::try_default().map_err(|e| TtsPlayerError::Playback(e.to_string()))?;

    let sink = rodio::Sink::try_new(&stream_handle)
        .map_err(|e| TtsPlayerError::Playback(e.to_string()))?;

    let source =
        rodio::Decoder::new(cursor).map_err(|e| TtsPlayerError::Playback(e.to_string()))?;

    sink.append(source);

    // Poll for stop or completion
    while !sink.empty() {
        if stop_flag.load(Ordering::Relaxed) {
            sink.stop();
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    sink.sleep_until_end();
    Ok(())
}

/// Split text into sentences at natural boundaries.
///
/// Splits on sentence-ending punctuation (`.`, `!`, `?`) followed by whitespace,
/// preserving the punctuation with the sentence.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);

        if (ch == '.' || ch == '!' || ch == '?') && !current.trim().is_empty() {
            // Check if this is a real sentence end (not an abbreviation)
            if is_sentence_end(&current) {
                sentences.push(current.trim().to_string());
                current = String::new();
            }
        }
    }

    // Push remaining text
    let remaining = current.trim().to_string();
    if !remaining.is_empty() {
        sentences.push(remaining);
    }

    sentences
}

/// Find the best sentence break point in text (for partial playback).
///
/// Returns the byte index after the last sentence-ending punctuation,
/// or None if no good break point found.
pub fn find_sentence_break(text: &str) -> Option<usize> {
    let mut last_break = None;

    for (i, ch) in text.char_indices() {
        if ch == '.' || ch == '!' || ch == '?' {
            // Verify it's followed by whitespace or end of string
            let next_idx = i + ch.len_utf8();
            if next_idx >= text.len() || text[next_idx..].starts_with(char::is_whitespace) {
                last_break = Some(next_idx);
            }
        }
    }

    last_break
}

/// Heuristic: check if the period is a real sentence end.
fn is_sentence_end(text: &str) -> bool {
    let trimmed = text.trim();

    // Skip common abbreviations
    let abbrevs = [
        "Mr.", "Mrs.", "Ms.", "Dr.", "Prof.", "Sr.", "Jr.", "vs.", "etc.", "e.g.", "i.e.",
    ];
    for abbrev in &abbrevs {
        if trimmed.ends_with(abbrev) {
            return false;
        }
    }

    // Skip single-letter abbreviations (e.g., "A.")
    if trimmed.len() >= 2 {
        let chars: Vec<char> = trimmed.chars().collect();
        let last = chars[chars.len() - 1]; // The period
        let prev = chars[chars.len() - 2];
        if last == '.' && prev.is_uppercase() && (chars.len() < 3 || chars[chars.len() - 3] == ' ')
        {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_simple_sentences() {
        let text = "Hello. How are you? I'm fine!";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0], "Hello.");
        assert_eq!(sentences[1], "How are you?");
        assert_eq!(sentences[2], "I'm fine!");
    }

    #[test]
    fn split_no_punctuation() {
        let text = "Hello world";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0], "Hello world");
    }

    #[test]
    fn split_empty() {
        let sentences = split_sentences("");
        assert!(sentences.is_empty());
    }

    #[test]
    fn split_trailing_text() {
        let text = "First sentence. Then some more";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0], "First sentence.");
        assert_eq!(sentences[1], "Then some more");
    }

    #[test]
    fn split_abbreviations() {
        let text = "Dr. Smith went home. He was tired.";
        let sentences = split_sentences(text);
        // "Dr." should not split; "home." and "tired." should
        assert_eq!(sentences.len(), 2);
        assert!(sentences[0].contains("Dr. Smith"));
    }

    #[test]
    fn find_break_basic() {
        assert_eq!(find_sentence_break("Hello. World"), Some(6));
        assert_eq!(find_sentence_break("No break here"), None);
    }

    #[test]
    fn find_break_multiple() {
        let text = "First. Second. Third";
        let brk = find_sentence_break(text).unwrap();
        assert_eq!(brk, 14); // After "Second." (byte index of char after '.')
    }

    #[test]
    fn find_break_end_of_string() {
        assert_eq!(find_sentence_break("Done."), Some(5));
    }

    #[test]
    fn tts_player_error_display() {
        let e = TtsPlayerError::Synthesis("timeout".into());
        assert_eq!(format!("{e}"), "TTS synthesis: timeout");
    }
}
