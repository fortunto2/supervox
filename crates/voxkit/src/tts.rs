//! Text-to-speech — OpenAI TTS API backend.

use thiserror::Error;

/// TTS voice options (OpenAI voices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsVoice {
    Alloy,
    Ash,
    Coral,
    Echo,
    Fable,
    Nova,
    Onyx,
    Sage,
    Shimmer,
}

impl TtsVoice {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Alloy => "alloy",
            Self::Ash => "ash",
            Self::Coral => "coral",
            Self::Echo => "echo",
            Self::Fable => "fable",
            Self::Nova => "nova",
            Self::Onyx => "onyx",
            Self::Sage => "sage",
            Self::Shimmer => "shimmer",
        }
    }
}

/// TTS errors.
#[derive(Debug, Error)]
pub enum TtsError {
    #[error("Request: {0}")]
    Request(String),
    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
}

/// OpenAI TTS client.
#[derive(Clone)]
pub struct OpenAiTts {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    voice: TtsVoice,
    speed: f32,
}

impl OpenAiTts {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "tts-1".to_string(),
            voice: TtsVoice::Nova,
            speed: 1.0,
        }
    }

    pub fn with_voice(mut self, voice: TtsVoice) -> Self {
        self.voice = voice;
        self
    }

    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Synthesize text → MP3 bytes.
    pub async fn speak(&self, text: &str) -> Result<Vec<u8>, TtsError> {
        let url = format!("{}/audio/speech", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "voice": self.voice.as_str(),
            "input": text,
            "speed": self.speed,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Request(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(TtsError::Api { status, body });
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| TtsError::Request(e.to_string()))
    }
}
