use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfiguration {
    pub provider: String,
    pub model: String,
}

impl ProviderConfiguration {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfiguration {
    #[serde(default = "default_text_configuration")]
    pub text: ProviderConfiguration,
    #[serde(default = "default_stt_configuration")]
    pub stt: ProviderConfiguration,
    #[serde(default = "default_tts_configuration")]
    pub tts: ProviderConfiguration,
}

impl Default for AppConfiguration {
    fn default() -> Self {
        Self {
            text: default_text_configuration(),
            stt: default_stt_configuration(),
            tts: default_tts_configuration(),
        }
    }
}

fn default_text_configuration() -> ProviderConfiguration {
    ProviderConfiguration::new("openrouter", "qwen/qwen3.6-flash")
}

fn default_stt_configuration() -> ProviderConfiguration {
    ProviderConfiguration::new("openrouter", "openai/whisper-large-v3-turbo")
}

fn default_tts_configuration() -> ProviderConfiguration {
    ProviderConfiguration::new("openrouter", "google/gemini-3.1-flash-tts-preview")
}
