//! OpenAI STT backend — gpt-4o-transcribe API.

use crate::stt::{SttBackend, SttError};
use crate::types::{AudioChunk, Transcript};

/// OpenAI speech-to-text client.
#[derive(Clone)]
pub struct OpenAiStt {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    language: String,
    prompt: Option<String>,
}

impl OpenAiStt {
    /// Create with API key. Uses default model `gpt-4o-transcribe`.
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-transcribe".to_string(),
            language: "en".to_string(),
            prompt: None,
        }
    }

    /// Custom base URL (for proxies, Azure, etc.).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    /// Override model (e.g., "whisper-1").
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Set language hint (ISO 639-1).
    pub fn with_language(mut self, lang: &str) -> Self {
        self.language = lang.to_string();
        self
    }

    /// Vocabulary prompt — helps with domain-specific terms.
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = Some(prompt.to_string());
        self
    }
}

impl OpenAiStt {
    /// Transcribe a file from raw bytes (batch upload).
    ///
    /// Sends the file directly to OpenAI's transcriptions API as multipart upload.
    /// Supports all formats the API accepts: WAV, MP3, M4A, FLAC, OGG, WebM.
    pub async fn transcribe_file_bytes(
        &self,
        bytes: &[u8],
        filename: &str,
        mime: &str,
    ) -> Result<Transcript, SttError> {
        let url = format!("{}/audio/transcriptions", self.base_url);
        let mut form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("language", self.language.clone())
            .text("response_format", response_format_for_model(&self.model))
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes.to_vec())
                    .file_name(filename.to_string())
                    .mime_str(mime)
                    .map_err(|e| SttError::Encoding(e.to_string()))?,
            );

        if let Some(p) = &self.prompt {
            form = form.text("prompt", p.clone());
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| SttError::Request(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(SttError::Api { status, body });
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SttError::Request(e.to_string()))?;

        let text = body["text"].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            return Err(SttError::Empty);
        }

        let duration = body["duration"].as_f64().unwrap_or(0.0);

        let segments = body["segments"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|s| crate::types::Segment {
                        start: s["start"].as_f64().unwrap_or(0.0),
                        end: s["end"].as_f64().unwrap_or(0.0),
                        text: s["text"].as_str().unwrap_or("").to_string(),
                        speaker: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let language = body["language"].as_str().map(String::from);

        Ok(Transcript {
            text,
            segments,
            language,
            duration_secs: duration,
        })
    }
}

#[async_trait::async_trait]
impl SttBackend for OpenAiStt {
    async fn transcribe(&self, audio: &AudioChunk) -> Result<Transcript, SttError> {
        self.transcribe_with_context(audio, None).await
    }

    async fn transcribe_with_context(
        &self,
        audio: &AudioChunk,
        context: Option<&str>,
    ) -> Result<Transcript, SttError> {
        // Encode audio as WAV
        let wav_bytes = encode_wav_bytes(audio)?;

        let url = format!("{}/audio/transcriptions", self.base_url);
        let mut form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("language", self.language.clone())
            .text("response_format", response_format_for_model(&self.model))
            .part(
                "file",
                reqwest::multipart::Part::bytes(wav_bytes)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")
                    .map_err(|e| SttError::Encoding(e.to_string()))?,
            );

        // Combine vocabulary prompt with context
        let prompt = match (&self.prompt, context) {
            (Some(p), Some(c)) => Some(format!("{p}\n\nContext: {c}")),
            (Some(p), None) => Some(p.clone()),
            (None, Some(c)) => Some(format!("Context: {c}")),
            (None, None) => None,
        };
        if let Some(p) = prompt {
            form = form.text("prompt", p);
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| SttError::Request(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(SttError::Api { status, body });
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SttError::Request(e.to_string()))?;

        let text = body["text"].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            return Err(SttError::Empty);
        }

        let duration = body["duration"]
            .as_f64()
            .unwrap_or(audio.duration_ms as f64 / 1000.0);

        let segments = body["segments"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|s| crate::types::Segment {
                        start: s["start"].as_f64().unwrap_or(0.0),
                        end: s["end"].as_f64().unwrap_or(0.0),
                        text: s["text"].as_str().unwrap_or("").to_string(),
                        speaker: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let language = body["language"].as_str().map(String::from);

        Ok(Transcript {
            text,
            segments,
            language,
            duration_secs: duration,
        })
    }

    fn name(&self) -> &str {
        "openai"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_stt_builder() {
        let stt = OpenAiStt::new("test-key")
            .with_language("ru")
            .with_model("whisper-1")
            .with_base_url("http://localhost:8080/v1")
            .with_prompt("SuperVox meeting");

        assert_eq!(stt.language, "ru");
        assert_eq!(stt.model, "whisper-1");
        assert_eq!(stt.base_url, "http://localhost:8080/v1");
        assert_eq!(stt.prompt, Some("SuperVox meeting".to_string()));
    }
}

/// gpt-4o-transcribe only supports "json"/"text"; whisper-1 supports "verbose_json".
fn response_format_for_model(model: &str) -> &'static str {
    if model.starts_with("gpt-") {
        "json"
    } else {
        "verbose_json"
    }
}

/// Encode AudioChunk as WAV bytes (16-bit PCM mono).
fn encode_wav_bytes(audio: &AudioChunk) -> Result<Vec<u8>, SttError> {
    use std::io::Cursor;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    let mut writer =
        hound::WavWriter::new(&mut cursor, spec).map_err(|e| SttError::Encoding(e.to_string()))?;
    for &sample in &audio.samples {
        let i16_val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer
            .write_sample(i16_val)
            .map_err(|e| SttError::Encoding(e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| SttError::Encoding(e.to_string()))?;
    Ok(cursor.into_inner())
}
