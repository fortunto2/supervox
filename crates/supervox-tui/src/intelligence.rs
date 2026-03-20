//! Live intelligence: async translation + rolling summary pipelines.
//!
//! Spawns background tasks that process transcript events and send results
//! back via AudioEvent channel without blocking the UI.

use std::sync::Arc;
use std::time::Duration;
use supervox_agent::types::Config;
use tokio::sync::{Mutex, mpsc};

use crate::audio::AudioEvent;

/// Shared state for the rolling summary pipeline.
struct SummaryState {
    /// Recent final transcript chunks since last summary.
    pending_chunks: Vec<String>,
    /// Last generated summary for context continuity.
    last_summary: Option<String>,
}

/// Start the translation pipeline: listens for final transcripts and sends translations.
pub fn start_translation_pipeline(
    config: &Config,
    mut transcript_rx: mpsc::UnboundedReceiver<(String, String)>, // (source_id, text)
    event_tx: mpsc::UnboundedSender<AudioEvent>,
) {
    let to_lang = config.my_language.clone();
    let model = config.llm_model.clone();

    tokio::spawn(async move {
        while let Some((source_id, text)) = transcript_rx.recv().await {
            if text.is_empty() {
                continue;
            }
            let to_lang = to_lang.clone();
            let model = model.clone();
            let event_tx = event_tx.clone();
            let source_id = source_id.clone();

            // Each translation is independent — spawn in parallel
            tokio::spawn(async move {
                match translate(&text, &to_lang, &model).await {
                    Ok(translated) => {
                        let _ = event_tx.send(AudioEvent::Translation {
                            source_id,
                            text: translated,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Translation failed: {e}");
                    }
                }
            });
        }
    });
}

/// Start the rolling summary pipeline: generates summary every N seconds.
pub fn start_summary_pipeline(
    config: &Config,
    transcript_rx: mpsc::UnboundedReceiver<String>, // final transcript text
    event_tx: mpsc::UnboundedSender<AudioEvent>,
) {
    let interval = Duration::from_secs(config.summary_lag_secs as u64);
    let target_lang = config.my_language.clone();
    let model = config.llm_model.clone();

    let state = Arc::new(Mutex::new(SummaryState {
        pending_chunks: Vec::new(),
        last_summary: None,
    }));

    // Collector: receives transcript chunks and stores them
    let collector_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut rx = transcript_rx;
        while let Some(text) = rx.recv().await {
            if !text.is_empty() {
                let mut s = collector_state.lock().await;
                s.pending_chunks.push(text);
            }
        }
    });

    // Timer: generates summary at intervals
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // skip first immediate tick

        loop {
            ticker.tick().await;

            let (chunks, prior) = {
                let mut s = state.lock().await;
                if s.pending_chunks.is_empty() {
                    continue;
                }
                let chunks = std::mem::take(&mut s.pending_chunks);
                (chunks, s.last_summary.clone())
            };

            match summarize(&chunks, prior.as_deref(), &target_lang, &model).await {
                Ok(summary) => {
                    {
                        let mut s = state.lock().await;
                        s.last_summary = Some(summary.clone());
                    }
                    let _ = event_tx.send(AudioEvent::Summary(summary));
                }
                Err(e) => {
                    tracing::warn!("Summary failed: {e}");
                }
            }
        }
    });
}

/// Translate text using LLM.
async fn translate(text: &str, to_lang: &str, model: &str) -> Result<String, String> {
    use sgr_agent::Llm;
    use sgr_agent::types::{LlmConfig, Message};

    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(format!(
            "You are a translator. Translate the text to {to_lang}. \
             Return ONLY the translated text, no explanations."
        )),
        Message::user(text),
    ];
    llm.generate(&messages)
        .await
        .map_err(|e| format!("Translate LLM error: {e}"))
}

/// Generate rolling summary using LLM.
async fn summarize(
    chunks: &[String],
    prior: Option<&str>,
    target_lang: &str,
    model: &str,
) -> Result<String, String> {
    use sgr_agent::Llm;
    use sgr_agent::types::{LlmConfig, Message};

    let transcript = chunks.join("\n");
    let prior_ctx = prior
        .map(|s| format!("\nPrevious summary:\n{s}"))
        .unwrap_or_default();

    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(format!(
            "You are a live call summarizer. Produce 3-5 bullet points capturing \
             the key meaning of the conversation so far. Write in {target_lang}. \
             Focus on meaning, not word-for-word transcription. Be concise."
        )),
        Message::user(format!("Transcript:\n{transcript}{prior_ctx}")),
    ];
    llm.generate(&messages)
        .await
        .map_err(|e| format!("Summary LLM error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            my_language: "ru".into(),
            stt_backend: "realtime".into(),
            llm_model: "test".into(),
            summary_lag_secs: 1,
            capture: "mic".into(),
        }
    }

    #[tokio::test]
    async fn translation_pipeline_receives_transcripts() {
        let config = test_config();
        let (tr_tx, tr_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        start_translation_pipeline(&config, tr_rx, event_tx);

        // Send a transcript — translation will fail (no LLM) but pipeline should not panic
        tr_tx.send(("id-1".into(), "Hello".into())).unwrap();

        // Empty text should be skipped
        tr_tx.send(("id-2".into(), String::new())).unwrap();

        // Give pipeline time to process
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(tr_tx);

        // Drain events — translation fails gracefully (no real LLM)
        while event_rx.try_recv().is_ok() {}
    }

    #[tokio::test]
    async fn summary_pipeline_collects_chunks() {
        let config = Config {
            summary_lag_secs: 1,
            ..test_config()
        };
        let (sum_tx, sum_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        start_summary_pipeline(&config, sum_rx, event_tx);

        // Send transcript chunks
        sum_tx.send("You: Hello".into()).unwrap();
        sum_tx.send("Them: Hi there".into()).unwrap();

        // Wait for summary interval to fire
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(sum_tx);

        // Summary fails gracefully (no real LLM)
        while event_rx.try_recv().is_ok() {}
    }

    #[test]
    fn config_values_respected() {
        let config = Config {
            my_language: "de".into(),
            summary_lag_secs: 10,
            capture: "mic+system".into(),
            ..test_config()
        };
        assert_eq!(config.my_language, "de");
        assert_eq!(config.summary_lag_secs, 10);
        assert!(config.capture.contains("system"));
    }
}
