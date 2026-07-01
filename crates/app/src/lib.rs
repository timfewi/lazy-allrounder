use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use lazy_allrounder_core::config::{
    AppConfiguration, HotkeysConfiguration, OverlayConfiguration, OverlayCorner,
    ProviderConfiguration,
};
use lazy_allrounder_core::error::{CoreError, PortError};
use lazy_allrounder_core::services::{
    AskRequest, GeneratedAudio, ReadRequest, ReadService, TransformService,
};
use lazy_allrounder_integrations::{
    OpenRouterClient, OpenRouterSpeechToTextClient, OpenRouterTextClient,
    OpenRouterTextToSpeechClient,
};
pub use lazy_allrounder_platform::{DictateState, DictateStatus};
use lazy_allrounder_platform::{
    DictateToggleResult, PendingDictation, capture_microphone_until_enter, dictate_start,
    dictate_status as platform_dictate_status, dictate_stop, dictate_toggle,
    insert_text_into_focused_app,
};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("could not determine the default configuration directory")]
    MissingProjectDirectory,
    #[error("failed to read configuration file at {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse configuration file at {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("config file does not exist at {0}; copy config.example.toml and edit it first")]
    MissingConfig(PathBuf),
    #[error("unsupported provider {provider:?} for {pipeline}")]
    UnsupportedProvider {
        pipeline: &'static str,
        provider: String,
    },
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("provider initialization failed: {0}")]
    Provider(String),
}

#[derive(Debug, Clone)]
pub struct LoadedConfiguration {
    pub path: PathBuf,
    pub config: AppConfiguration,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct Application {
    config: AppConfiguration,
    stt_client: OpenRouterSpeechToTextClient,
    text_client: OpenRouterTextClient,
    tts_client: OpenRouterTextToSpeechClient,
}

#[derive(Debug)]
pub enum DictateCaptureOutcome {
    Started,
    Pending(PendingDictation),
}

impl Application {
    pub fn from_loaded_configuration(loaded: &LoadedConfiguration) -> Result<Self, AppError> {
        if !loaded.exists {
            return Err(AppError::MissingConfig(loaded.path.clone()));
        }

        let config = loaded.config.clone();
        ensure_provider("text", &config.text.provider)?;
        ensure_provider("stt", &config.stt.provider)?;
        ensure_provider("tts", &config.tts.provider)?;

        let shared_client =
            OpenRouterClient::from_env().map_err(|error| AppError::Provider(error.to_string()))?;
        let tts_voice = config.tts.voice.clone();

        Ok(Self {
            config,
            stt_client: OpenRouterSpeechToTextClient::new(shared_client.clone()),
            text_client: OpenRouterTextClient::new(shared_client.clone()),
            tts_client: OpenRouterTextToSpeechClient::with_voice(shared_client, tts_voice),
        })
    }

    pub async fn dictate(&self, audio: Vec<u8>, format: &'static str) -> Result<String, AppError> {
        if audio.is_empty() {
            return Err(AppError::Core(CoreError::EmptyInput));
        }

        self.stt_client
            .transcribe(&audio, format, &self.config.stt.model)
            .await
            .map_err(CoreError::from)
            .map_err(AppError::from)
    }

    pub async fn dictate_from_microphone(&self) -> Result<String, AppError> {
        let pending = capture_microphone_until_enter()
            .map_err(CoreError::from)
            .map_err(AppError::from)?;

        self.transcribe_pending_dictation(pending).await
    }

    pub fn insert_dictated_text(&self, transcript: &str) -> Result<(), AppError> {
        insert_text_into_focused_app(transcript)
            .map_err(CoreError::from)
            .map_err(AppError::from)
    }

    pub async fn read(&self, text: String) -> Result<Vec<u8>, AppError> {
        let service = self.read_service();
        let request = ReadRequest::new(text)?;

        service
            .run(request, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn explain(&self, text: String) -> Result<GeneratedAudio, AppError> {
        let service = self.transform_service();
        let request = ReadRequest::new(text)?;

        service
            .explain(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn summarize(&self, text: String) -> Result<GeneratedAudio, AppError> {
        let service = self.transform_service();
        let request = ReadRequest::new(text)?;

        service
            .summarize(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn ask(&self, text: String, question: String) -> Result<GeneratedAudio, AppError> {
        let service = self.transform_service();
        let request = AskRequest::new(text, question)?;

        service
            .ask(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn transcribe_pending_dictation(
        &self,
        pending: PendingDictation,
    ) -> Result<String, AppError> {
        let (audio, completion) = pending.into_parts();
        let result = self.dictate(audio, "wav").await;
        let finish_result = completion
            .finish()
            .map_err(CoreError::from)
            .map_err(AppError::from);

        match (result, finish_result) {
            (Ok(transcript), Ok(())) => Ok(transcript),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn read_service(&self) -> ReadService<OpenRouterTextToSpeechClient> {
        ReadService::new(self.tts_client.clone())
    }

    fn transform_service(
        &self,
    ) -> TransformService<OpenRouterTextClient, OpenRouterTextToSpeechClient> {
        TransformService::new(self.text_client.clone(), self.tts_client.clone())
    }
}

pub fn dictate_start_capture() -> Result<(), AppError> {
    map_port_result(dictate_start())
}

pub fn dictate_stop_capture() -> Result<PendingDictation, AppError> {
    map_port_result(dictate_stop())
}

pub fn dictate_toggle_capture() -> Result<DictateCaptureOutcome, AppError> {
    match map_port_result(dictate_toggle())? {
        DictateToggleResult::Started => Ok(DictateCaptureOutcome::Started),
        DictateToggleResult::Pending(pending) => Ok(DictateCaptureOutcome::Pending(pending)),
    }
}

pub fn dictate_runtime_status() -> Result<DictateStatus, AppError> {
    map_port_result(platform_dictate_status())
}

pub fn default_config_path() -> Result<PathBuf, AppError> {
    let Some(project_dirs) = ProjectDirs::from("", "", "lazy-allrounder") else {
        return Err(AppError::MissingProjectDirectory);
    };

    Ok(project_dirs.config_dir().join("config.toml"))
}

pub fn load_configuration(path: Option<&Path>) -> Result<LoadedConfiguration, AppError> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => default_config_path()?,
    };

    if !path.exists() {
        return Ok(LoadedConfiguration {
            path,
            config: AppConfiguration::default(),
            exists: false,
        });
    }

    let raw = fs::read_to_string(&path).map_err(|source| AppError::ReadConfig {
        path: path.clone(),
        source,
    })?;
    let config = parse_configuration(&raw, &path)?;

    Ok(LoadedConfiguration {
        path,
        config,
        exists: true,
    })
}

pub fn parse_configuration(raw: &str, path: &Path) -> Result<AppConfiguration, AppError> {
    let parsed: RawAppConfiguration =
        toml::from_str(raw).map_err(|source| AppError::ParseConfig {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(parsed.merge_with_defaults())
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderConfiguration {
    provider: Option<String>,
    model: Option<String>,
    voice: Option<String>,
}

impl RawProviderConfiguration {
    fn merge_with_defaults(self, defaults: ProviderConfiguration) -> ProviderConfiguration {
        // A user-picked model gets a user-picked (or unset) voice: falling
        // back to the default voice would pair it with the wrong model.
        let voice = if self.model.is_some() {
            self.voice
        } else {
            self.voice.or(defaults.voice)
        };

        ProviderConfiguration {
            provider: self.provider.unwrap_or(defaults.provider),
            model: self.model.unwrap_or(defaults.model),
            voice,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverlayConfiguration {
    enabled: Option<bool>,
    corner: Option<OverlayCorner>,
}

impl RawOverlayConfiguration {
    fn merge_with_defaults(self, defaults: OverlayConfiguration) -> OverlayConfiguration {
        OverlayConfiguration {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            corner: self.corner.unwrap_or(defaults.corner),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHotkeysConfiguration {
    read: Option<String>,
    summarize: Option<String>,
    explain: Option<String>,
    ask: Option<String>,
    dictate: Option<String>,
}

impl RawHotkeysConfiguration {
    fn merge_with_defaults(self, defaults: HotkeysConfiguration) -> HotkeysConfiguration {
        HotkeysConfiguration {
            read: self.read.unwrap_or(defaults.read),
            summarize: self.summarize.unwrap_or(defaults.summarize),
            explain: self.explain.unwrap_or(defaults.explain),
            ask: self.ask.unwrap_or(defaults.ask),
            dictate: self.dictate.unwrap_or(defaults.dictate),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAppConfiguration {
    text: Option<RawProviderConfiguration>,
    stt: Option<RawProviderConfiguration>,
    tts: Option<RawProviderConfiguration>,
    overlay: Option<RawOverlayConfiguration>,
    hotkeys: Option<RawHotkeysConfiguration>,
}

impl RawAppConfiguration {
    fn merge_with_defaults(self) -> AppConfiguration {
        let defaults = AppConfiguration::default();

        AppConfiguration {
            text: self
                .text
                .unwrap_or_default()
                .merge_with_defaults(defaults.text),
            stt: self
                .stt
                .unwrap_or_default()
                .merge_with_defaults(defaults.stt),
            tts: self
                .tts
                .unwrap_or_default()
                .merge_with_defaults(defaults.tts),
            overlay: self
                .overlay
                .unwrap_or_default()
                .merge_with_defaults(defaults.overlay),
            hotkeys: self
                .hotkeys
                .unwrap_or_default()
                .merge_with_defaults(defaults.hotkeys),
        }
    }
}

fn ensure_provider(pipeline: &'static str, provider: &str) -> Result<(), AppError> {
    if provider.eq_ignore_ascii_case("openrouter") {
        return Ok(());
    }

    Err(AppError::UnsupportedProvider {
        pipeline,
        provider: provider.to_owned(),
    })
}

fn map_port_result<T>(result: Result<T, PortError>) -> Result<T, AppError> {
    result.map_err(CoreError::from).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse_configuration;

    #[test]
    fn parses_partial_toml_and_keeps_defaults() {
        let config = parse_configuration(
            r#"
            [text]
            model = "custom-model"
            "#,
            Path::new("config.toml"),
        )
        .expect("partial config should parse");

        assert_eq!(config.text.model, "custom-model");
        assert_eq!(config.stt.provider, "openrouter");
        assert_eq!(config.tts.provider, "openrouter");
    }

    #[test]
    fn rejects_unknown_top_level_keys() {
        let error = parse_configuration(
            r#"
            [shortcuts]
            read = "Super+S"
            "#,
            Path::new("config.toml"),
        )
        .expect_err("unknown keys should fail");

        assert!(matches!(error, super::AppError::ParseConfig { .. }));
    }

    #[test]
    fn parses_hotkeys_section_and_keeps_defaults() {
        let config = parse_configuration(
            r#"
            [hotkeys]
            read = "ctrl+shift+r"
            dictate = ""
            "#,
            Path::new("config.toml"),
        )
        .expect("hotkeys config should parse");

        assert_eq!(config.hotkeys.read, "ctrl+shift+r");
        assert_eq!(config.hotkeys.summarize, "super+w");
        // An empty binding disables the action.
        assert!(
            !config
                .hotkeys
                .enabled_bindings()
                .iter()
                .any(|(action, _)| action == "dictate")
        );
    }

    #[test]
    fn parses_tts_voice_and_defaults_pair_model_with_voice() {
        let config = parse_configuration(
            r#"
            [tts]
            model = "zyphra/zonos-v0.1-transformer"
            voice = "american_female"
            "#,
            Path::new("config.toml"),
        )
        .expect("tts voice should parse");
        assert_eq!(config.tts.voice.as_deref(), Some("american_female"));

        // Defaults stay paired: no [tts] section keeps kokoro + its voice.
        let defaults =
            parse_configuration("", Path::new("config.toml")).expect("empty config parses");
        assert_eq!(defaults.tts.model, "hexgrad/kokoro-82m");
        assert_eq!(defaults.tts.voice.as_deref(), Some("af_heart"));

        // A custom model without a voice must NOT inherit the default voice.
        let custom = parse_configuration(
            r#"
            [tts]
            model = "some/other-model"
            "#,
            Path::new("config.toml"),
        )
        .expect("custom model parses");
        assert_eq!(custom.tts.voice, None);
    }

    #[test]
    fn parses_overlay_section_and_keeps_defaults() {
        let config = parse_configuration(
            r#"
            [overlay]
            corner = "top-left"
            "#,
            Path::new("config.toml"),
        )
        .expect("overlay config should parse");

        assert!(config.overlay.enabled);
        assert_eq!(
            config.overlay.corner,
            lazy_allrounder_core::config::OverlayCorner::TopLeft
        );
    }

    #[test]
    fn rejects_invalid_overlay_corner() {
        let error = parse_configuration(
            r#"
            [overlay]
            corner = "middle"
            "#,
            Path::new("config.toml"),
        )
        .expect_err("invalid corner should fail");

        assert!(matches!(error, super::AppError::ParseConfig { .. }));
    }
}
