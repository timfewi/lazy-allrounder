use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfiguration {
    pub provider: String,
    pub model: String,
    /// Provider-specific voice name; only meaningful for the TTS pipeline
    /// (e.g. grok-voice's "eve"). None lets the provider client choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Speaking speed multiplier; only meaningful for the TTS pipeline.
    /// None lets the provider use its default pace (1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}

impl ProviderConfiguration {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            voice: None,
            speed: None,
        }
    }

    pub fn with_voice(provider: &str, model: &str, voice: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            voice: Some(voice.to_owned()),
            speed: None,
        }
    }
}

/// The widest range of speaking speeds the OpenAI-compatible speech endpoint
/// accepts; config values are clamped into it before use. Individual models are
/// stricter — the default x-ai/grok-voice-tts-1.0 rejects anything outside
/// 0.7..=1.5 — so a speed valid here can still be a 400 at the provider. Kept
/// wide because the clamp is shared by every TTS model.
pub const TTS_SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// Clamps a configured speaking speed into [`TTS_SPEED_RANGE`], mapping
/// non-finite values to the provider default (None).
pub fn clamp_tts_speed(speed: Option<f32>) -> Option<f32> {
    speed
        .filter(|speed| speed.is_finite())
        .map(|speed| speed.clamp(*TTS_SPEED_RANGE.start(), *TTS_SPEED_RANGE.end()))
}

/// A speed as it should be written to the config file: quantized to two
/// decimals in f64 so a raw f32 never drags float dust into the TOML
/// (1.3f32 as f64 is 1.2999999…).
pub fn tts_speed_file_value(speed: f32) -> f64 {
    (f64::from(speed) * 100.0).round() / 100.0
}

/// The same two-decimal grid as [`tts_speed_file_value`], as f32 — the GUI
/// slider and the persisted value must share one quantization, or a stored
/// speed re-read from disk would register as a phantom "change".
pub fn round_tts_speed(speed: f32) -> f32 {
    tts_speed_file_value(speed) as f32
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    // grok-voice covers 20+ languages and detects the input language itself,
    // so the same voice reads English and German. kokoro is cheaper but its
    // eight languages exclude German, and it would phonemize it as English.
    ProviderConfiguration::with_voice("openrouter", "x-ai/grok-voice-tts-1.0", "eve")
}
