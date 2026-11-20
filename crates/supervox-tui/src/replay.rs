//! `supervox replay <wav>`: feed a recording through the live STT pipeline as if it were the
//! microphone, and print every final line with the label the live panel would show.
//!
//! Same path as `supervox live`: `create_stt_backend(config)` then `connect()`, chunks of
//! 100 ms resampled to 24 kHz i16 like `run_pipeline` does. Only the capture is replaced.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use supervox_agent::types::Config;
use voxkit::stt::{SttInput, TranscriptEvent};

use crate::audio::{AudioSource, create_stt_backend, speaker_label};

/// One final line as the live panel would show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub label: String,
    pub span: Option<(f32, f32)>,
    pub text: String,
}

/// Run `wav[from..to]` (seconds; `to` None = to the end) through the live backend.
pub async fn replay(
    config: &Config,
    wav: &Path,
    from: f32,
    to: Option<f32>,
) -> Result<(Vec<Line>, f32)> {
    let chunk = voxkit::read_wav_file(wav).map_err(|e| anyhow::anyhow!("read wav: {e}"))?;
    let rate = chunk.sample_rate as usize;
    let a = ((from * rate as f32) as usize).min(chunk.samples.len());
    let b = to
        .map_or(chunk.samples.len(), |t| (t * rate as f32) as usize)
        .clamp(a, chunk.samples.len());
    let samples = &chunk.samples[a..b];

    let backend = create_stt_backend(config).map_err(|e| anyhow::anyhow!(e))?;
    let t = Instant::now();
    let (tx, mut rx) = backend.connect().await.context("STT connect")?;
    let ready = t.elapsed().as_secs_f32();

    let agent = config.agent_name.clone();
    let collector = tokio::spawn(async move {
        let mut lines = Vec::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                TranscriptEvent::Final {
                    text,
                    speaker,
                    span,
                    ..
                } if !text.is_empty() => lines.push(Line {
                    label: speaker_label(&AudioSource::Mic, speaker.as_ref(), &agent),
                    span,
                    text,
                }),
                TranscriptEvent::Error(e) => eprintln!("STT error: {e}"),
                _ => {}
            }
        }
        lines
    });

    for c in samples.chunks(rate / 10) {
        let _ = tx
            .send(SttInput::Audio(voxkit::realtime_stt::resample_to_24k(
                c,
                chunk.sample_rate,
            )))
            .await;
    }
    let _ = tx.send(SttInput::Close).await;
    drop(tx);
    let lines = collector.await.context("collect transcript")?;
    Ok((lines, ready))
}

/// CLI entry: JSON lines on stdout (`label`, `a`, `b`, `text`), time-to-ready on stderr.
pub async fn cmd(config: &Config, wav: &str, from: f32, to: Option<f32>) -> Result<()> {
    // RUST_LOG=voxkit=debug shows each segment's voiceprint similarity.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let (lines, ready) = replay(config, Path::new(wav), from, to).await?;
    for l in &lines {
        println!(
            "{}",
            serde_json::json!({
                "label": l.label,
                "a": l.span.map(|s| s.0),
                "b": l.span.map(|s| s.1),
                "text": l.text,
            })
        );
    }
    eprintln!("ready in {ready:.2}s, {} lines", lines.len());
    Ok(())
}
