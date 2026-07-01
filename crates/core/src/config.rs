use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfiguration {
    pub provider: String,
    pub model: String,
    /// Provider-specific voice name; only meaningful for the TTS pipeline
    /// (e.g. kokoro's "af_heart"). None lets the provider client choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

impl ProviderConfiguration {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            voice: None,
        }
    }

    pub fn with_voice(provider: &str, model: &str, voice: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            voice: Some(voice.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayConfiguration {
    pub enabled: bool,
    pub corner: OverlayCorner,
}

impl Default for OverlayConfiguration {
    fn default() -> Self {
        Self {
            enabled: true,
            corner: OverlayCorner::BottomRight,
        }
    }
}

/// Global hotkey bindings per action. An empty string disables the binding.
/// Defaults mirror the GNOME shortcut scheme this app replaces, so existing
/// muscle memory carries over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeysConfiguration {
    pub read: String,
    pub summarize: String,
    pub explain: String,
    pub ask: String,
    pub dictate: String,
}

impl Default for HotkeysConfiguration {
    fn default() -> Self {
        Self {
            read: "super+s".to_owned(),
            summarize: "super+w".to_owned(),
            explain: "super+a".to_owned(),
            ask: "super+shift+a".to_owned(),
            dictate: "super+d".to_owned(),
        }
    }
}

impl HotkeysConfiguration {
    /// The (action, binding) pairs that are actually enabled.
    pub fn enabled_bindings(&self) -> Vec<(String, String)> {
        [
            ("read", &self.read),
            ("summarize", &self.summarize),
            ("explain", &self.explain),
            ("ask", &self.ask),
            ("dictate", &self.dictate),
        ]
        .into_iter()
        .filter(|(_, binding)| !binding.trim().is_empty())
        .map(|(action, binding)| (action.to_owned(), binding.clone()))
        .collect()
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
    #[serde(default)]
    pub overlay: OverlayConfiguration,
    #[serde(default)]
    pub hotkeys: HotkeysConfiguration,
}

impl Default for AppConfiguration {
    fn default() -> Self {
        Self {
            text: default_text_configuration(),
            stt: default_stt_configuration(),
            tts: default_tts_configuration(),
            overlay: OverlayConfiguration::default(),
            hotkeys: HotkeysConfiguration::default(),
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
    // google/gemini-3.1-flash-tts-preview was removed from OpenRouter and
    // now returns 400; kokoro is the known-good hosted TTS default.
    ProviderConfiguration::with_voice("openrouter", "hexgrad/kokoro-82m", "af_heart")
}
