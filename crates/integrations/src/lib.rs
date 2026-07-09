use std::env;
use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lazy_allrounder_core::{
    error::PortError,
    ports::{TextGenerationPort, TextOperation, TextToSpeechPort},
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

// Matches the default TTS model (hexgrad/kokoro-82m) in core's config.
const DEFAULT_TTS_VOICE: &str = "af_heart";
const DEFAULT_TTS_RESPONSE_FORMAT: &str = "mp3";
// Sent when no speed is configured: pins the pace at 1.0x instead of
// delegating to the provider's default, which could drift between releases.
const DEFAULT_TTS_SPEED: f32 = 1.0;

#[derive(Debug, Clone)]
pub struct OpenRouterClient {
    http_client: Client,
    api_key: String,
    http_referer: Option<String>,
}

impl Default for OpenRouterClient {
    fn default() -> Self {
        Self {
            http_client: Client::builder()
                .timeout(Duration::from_secs(60))
                .user_agent("lazy-allrounder/0.1.0")
                .build()
                .expect("reqwest client should build"),
            api_key: env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            http_referer: env::var("LAZY_ALLROUNDER_HTTP_REFERER").ok(),
        }
    }
}

impl OpenRouterClient {
    pub fn from_env() -> Result<Self, PortError> {
        let client = Self::default();

        if client.api_key.is_empty() {
            return Err(PortError::Other {
                message: "OPENROUTER_API_KEY is not set.".to_owned(),
            });
        }

        Ok(client)
    }

    /// Builds a client with an explicit key (e.g. loaded from the OS
    /// keyring); the caller owns the resolution order.
    pub fn with_api_key(api_key: String) -> Result<Self, PortError> {
        if api_key.trim().is_empty() {
            return Err(PortError::Other {
                message: "the OpenRouter API key is empty.".to_owned(),
            });
        }

        Ok(Self {
            api_key,
            ..Self::default()
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("https://openrouter.ai/api/v1{path}");
        let builder = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("X-Title", "lazy-allrounder");

        match &self.http_referer {
            Some(http_referer) => builder.header("HTTP-Referer", http_referer),
            None => builder,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouterTextClient {
    client: OpenRouterClient,
}

impl OpenRouterTextClient {
    pub fn new(client: OpenRouterClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl TextGenerationPort for OpenRouterTextClient {
    async fn generate(
        &self,
        input: &str,
        operation: &TextOperation,
        model: &str,
    ) -> Result<String, PortError> {
        let request = ChatCompletionRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system_prompt(operation),
                },
                ChatMessage {
                    role: "user",
                    content: user_prompt(input, operation),
                },
            ],
            temperature: Some(0.2),
        };

        let response = self
            .client
            .post("/chat/completions")
            .json(&request)
            .send()
            .await
            .map_err(map_request_error)?;
        let status = response.status();

        if !status.is_success() {
            return Err(error_from_response(status, response).await);
        }

        let parsed: ChatCompletionResponse = response.json().await.map_err(map_request_error)?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or(PortError::MalformedResponse)?;

        extract_message_text(choice.message)
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouterTextToSpeechClient {
    client: OpenRouterClient,
    voice: Option<String>,
    speed: Option<f32>,
}

impl OpenRouterTextToSpeechClient {
    pub fn new(client: OpenRouterClient) -> Self {
        Self {
            client,
            voice: None,
            speed: None,
        }
    }

    /// Voice is provider- and model-specific (kokoro wants "af_heart",
    /// OpenAI-style models want "alloy"), and speed is a user preference, so
    /// both are set per client rather than threaded through the domain port.
    /// A `None` speed sends [`DEFAULT_TTS_SPEED`]; values are clamped to the
    /// endpoint's accepted range.
    pub fn with_voice(client: OpenRouterClient, voice: Option<String>, speed: Option<f32>) -> Self {
        Self {
            client,
            voice,
            speed: lazy_allrounder_core::config::clamp_tts_speed(speed),
        }
    }
}

#[async_trait]
impl TextToSpeechPort for OpenRouterTextToSpeechClient {
    async fn synthesize(&self, text: &str, model: &str) -> Result<Vec<u8>, PortError> {
        let request = TextToSpeechRequest {
            model,
            input: text,
            voice: self.voice.as_deref().unwrap_or(DEFAULT_TTS_VOICE),
            response_format: DEFAULT_TTS_RESPONSE_FORMAT,
            speed: Some(self.speed.unwrap_or(DEFAULT_TTS_SPEED)),
        };

        let response = self
            .client
            .post("/audio/speech")
            .json(&request)
            .send()
            .await
            .map_err(map_request_error)?;
        let status = response.status();

        if !status.is_success() {
            return Err(error_from_response(status, response).await);
        }

        let bytes = response.bytes().await.map_err(map_request_error)?;
        Ok(bytes.to_vec())
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouterSpeechToTextClient {
    client: OpenRouterClient,
}

impl OpenRouterSpeechToTextClient {
    pub fn new(client: OpenRouterClient) -> Self {
        Self { client }
    }

    pub async fn transcribe(
        &self,
        audio: &[u8],
        format: &'static str,
        model: &str,
    ) -> Result<String, PortError> {
        let request = SpeechToTextRequest {
            model,
            input_audio: InputAudio {
                data: STANDARD.encode(audio),
                format,
            },
        };

        let response = self
            .client
            .post("/audio/transcriptions")
            .json(&request)
            .send()
            .await
            .map_err(map_request_error)?;
        let status = response.status();

        if !status.is_success() {
            return Err(error_from_response(status, response).await);
        }

        let parsed: SpeechToTextResponse = response.json().await.map_err(map_request_error)?;
        Ok(parsed.text)
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: Option<ChatContent>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Deserialize)]
struct ChatContentPart {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Serialize)]
struct TextToSpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    response_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f32>,
}

#[derive(Debug, Serialize)]
struct SpeechToTextRequest<'a> {
    model: &'a str,
    input_audio: InputAudio,
}

#[derive(Debug, Serialize)]
struct InputAudio {
    data: String,
    format: &'static str,
}

#[derive(Debug, Deserialize)]
struct SpeechToTextResponse {
    text: String,
}

fn system_prompt(operation: &TextOperation) -> String {
    match operation {
        TextOperation::Explain => {
            "Explain the user text clearly and accurately. Keep it concise but useful.".to_owned()
        }
        TextOperation::Summarize => {
            "Summarize the user text faithfully. Preserve the important points and remove repetition."
                .to_owned()
        }
        TextOperation::Ask { .. } => {
            "Answer the user's question using the provided source text. If the source text is insufficient, say so plainly."
                .to_owned()
        }
    }
}

fn user_prompt(input: &str, operation: &TextOperation) -> String {
    match operation {
        TextOperation::Explain => format!("Explain this text:\n\n{input}"),
        TextOperation::Summarize => format!("Summarize this text:\n\n{input}"),
        TextOperation::Ask { question } => {
            format!("Source text:\n{input}\n\nQuestion:\n{question}")
        }
    }
}

fn extract_message_text(message: ChatResponseMessage) -> Result<String, PortError> {
    let Some(content) = message.content else {
        return Err(PortError::MalformedResponse);
    };

    match content {
        ChatContent::Text(text) if !text.trim().is_empty() => Ok(text),
        ChatContent::Text(_) => Err(PortError::MalformedResponse),
        ChatContent::Parts(parts) => {
            let text = parts
                .into_iter()
                .filter(|part| part.kind == "text")
                .filter_map(|part| part.text)
                .collect::<Vec<_>>()
                .join("");

            if text.trim().is_empty() {
                return Err(PortError::MalformedResponse);
            }

            Ok(text)
        }
    }
}

fn map_request_error(error: reqwest::Error) -> PortError {
    if error.is_timeout() {
        return PortError::Timeout;
    }

    PortError::Other {
        message: "OpenRouter request failed.".to_owned(),
    }
}

fn map_status_error(status: StatusCode) -> PortError {
    match status {
        StatusCode::UNAUTHORIZED => PortError::Authentication,
        StatusCode::TOO_MANY_REQUESTS => PortError::RateLimited,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            PortError::ProviderUnavailable
        }
        _ if status.is_server_error() => PortError::ProviderUnavailable,
        _ => PortError::Other {
            message: format!("OpenRouter request failed with status {status}."),
        },
    }
}

/// Like `map_status_error`, but includes OpenRouter's error message for the
/// catch-all case — a bare "400 Bad Request" hides the actual cause (wrong
/// model slug, invalid voice, malformed input).
async fn error_from_response(status: StatusCode, response: reqwest::Response) -> PortError {
    let fallback = map_status_error(status);
    if !matches!(fallback, PortError::Other { .. }) {
        return fallback;
    }

    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<OpenRouterErrorResponse>(&body)
        .map(|parsed| parsed.error.message)
        .unwrap_or(body);
    let detail: String = detail.chars().take(300).collect();

    if detail.trim().is_empty() {
        return fallback;
    }

    PortError::Other {
        message: format!("OpenRouter request failed with status {status}: {detail}"),
    }
}

#[derive(Debug, Deserialize)]
struct OpenRouterErrorResponse {
    error: OpenRouterErrorDetail,
}

#[derive(Debug, Deserialize)]
struct OpenRouterErrorDetail {
    message: String,
}
