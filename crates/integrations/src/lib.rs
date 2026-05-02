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

const DEFAULT_TTS_VOICE: &str = "alloy";
const DEFAULT_TTS_RESPONSE_FORMAT: &str = "mp3";

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
            return Err(map_status_error(status));
        }

        let parsed: ChatCompletionResponse = response.json().await.map_err(map_request_error)?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| PortError::MalformedResponse)?;

        extract_message_text(choice.message)
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouterTextToSpeechClient {
    client: OpenRouterClient,
}

impl OpenRouterTextToSpeechClient {
    pub fn new(client: OpenRouterClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl TextToSpeechPort for OpenRouterTextToSpeechClient {
    async fn synthesize(&self, text: &str, model: &str) -> Result<Vec<u8>, PortError> {
        let request = TextToSpeechRequest {
            model,
            input: text,
            voice: DEFAULT_TTS_VOICE,
            response_format: DEFAULT_TTS_RESPONSE_FORMAT,
            speed: Some(1.0),
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
            return Err(map_status_error(status));
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
            return Err(map_status_error(status));
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
    voice: &'static str,
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
