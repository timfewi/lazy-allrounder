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
// Re-exported for the CLI, which talks to the running GUI (and notifies the
// desktop) without depending on the platform crate directly.
use lazy_allrounder_platform::{
    DictateToggleResult, PendingDictation, capture_microphone_until_enter, dictate_start,
    dictate_status as platform_dictate_status, dictate_stop, dictate_toggle,
    insert_text_into_focused_app,
};
pub use lazy_allrounder_platform::{GuiAction, GuiCommand, SendError, notify, send_gui_command};
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
    #[error(
        "no OpenRouter API key found — set OPENROUTER_API_KEY, point \
         OPENROUTER_API_KEY_FILE at a file containing the key, or save a key \
         in the app panel"
    )]
    MissingApiKey,
    #[error("failed to write configuration file at {path}: {source}")]
    WriteConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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

        let api_key = resolve_api_key().ok_or(AppError::MissingApiKey)?;
        let shared_client = OpenRouterClient::with_api_key(api_key)
            .map_err(|error| AppError::Provider(error.to_string()))?;
        let tts_voice = config.tts.voice.clone();
        let tts_speed = config.tts.speed;

        Ok(Self {
            config,
            stt_client: OpenRouterSpeechToTextClient::new(shared_client.clone()),
            text_client: OpenRouterTextClient::new(shared_client.clone()),
            tts_client: OpenRouterTextToSpeechClient::with_voice(
                shared_client,
                tts_voice,
                tts_speed,
            ),
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

/// The OpenRouter API key, if one is available: the environment variable
/// wins (power users, scripts, CI), then a key file named by
/// `OPENROUTER_API_KEY_FILE` (secret managers like agenix/sops expose
/// secrets as root-owned-store-free files), then the OS keyring (saved
/// through the app panel's onboarding).
pub fn resolve_api_key() -> Option<String> {
    resolve_api_key_from(
        std::env::var("OPENROUTER_API_KEY").ok(),
        std::env::var("OPENROUTER_API_KEY_FILE").ok(),
        |path| fs::read_to_string(path),
        || match lazy_allrounder_platform::load_api_key() {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!("could not check the OS keyring for an API key: {error}");
                None
            }
        },
    )
}

/// Pure resolution order behind [`resolve_api_key`]. A key file that cannot
/// be read or is empty logs and falls through to the keyring: an unreadable
/// secret path (agenix not rebuilt yet, wrong machine) must degrade to the
/// other sources, not disable them.
fn resolve_api_key_from(
    env_key: Option<String>,
    key_file: Option<String>,
    read_file: impl Fn(&str) -> std::io::Result<String>,
    keyring: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if let Some(from_env) = env_key
        && !from_env.trim().is_empty()
    {
        return Some(from_env);
    }

    if let Some(path) = key_file.filter(|path| !path.trim().is_empty()) {
        match read_file(&path) {
            Ok(content) => {
                let key = content.trim();
                if key.is_empty() {
                    tracing::warn!("the key file at {path} is empty, trying the keyring");
                } else {
                    return Some(key.to_owned());
                }
            }
            Err(error) => {
                tracing::warn!("could not read the key file at {path}: {error}");
            }
        }
    }

    keyring()
}

/// Saves the key to the OS keyring for future runs.
pub fn store_api_key(api_key: &str) -> Result<(), AppError> {
    lazy_allrounder_platform::store_api_key(api_key)
        .map_err(|error| AppError::Provider(error.to_string()))
}

/// Persists the speaking-speed preference into the config file (created from
/// defaults first if missing), so the next `Application` picks it up.
///
/// Edits only `[tts].speed` in the parsed document: entries the user never
/// wrote stay absent (so future default changes still reach them) and their
/// own entries keep their values. Comments do not survive the rewrite — the
/// same trade-off `ensure_configuration_file` makes when generating the file.
/// The write goes through a sibling temp file + rename, so a crash mid-write
/// can never truncate the config.
pub fn store_tts_speed(speed: f32) -> Result<(), AppError> {
    let loaded = ensure_configuration_file(None)?;
    let path = loaded.path;

    let raw = fs::read_to_string(&path).map_err(|source| AppError::ReadConfig {
        path: path.clone(),
        source,
    })?;
    let mut document: toml::Table =
        toml::from_str(&raw).map_err(|source| AppError::ParseConfig {
            path: path.clone(),
            source,
        })?;

    let tts = document
        .entry("tts")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !tts.is_table() {
        // Cannot happen after a successful parse above, but never destroy
        // unexpected content silently.
        *tts = toml::Value::Table(toml::Table::new());
    }
    let tts = tts.as_table_mut().expect("just ensured a table");
    match lazy_allrounder_core::config::clamp_tts_speed(Some(speed)) {
        Some(speed) => {
            tts.insert(
                "speed".to_owned(),
                toml::Value::Float(lazy_allrounder_core::config::tts_speed_file_value(speed)),
            );
        }
        None => {
            tts.remove("speed");
        }
    }

    let serialized =
        toml::to_string_pretty(&document).expect("a parsed toml table always serializes");
    let temp_path = path.with_extension("toml.tmp");
    fs::write(&temp_path, serialized).map_err(|source| AppError::WriteConfig {
        path: temp_path.clone(),
        source,
    })?;
    fs::rename(&temp_path, &path).map_err(|source| AppError::WriteConfig { path, source })
}

/// Loads the configuration, writing a default config file first if none
/// exists — so a fresh GUI install works without hand-editing TOML. The CLI
/// keeps using `load_configuration` directly and stays strict.
pub fn ensure_configuration_file(path: Option<&Path>) -> Result<LoadedConfiguration, AppError> {
    let loaded = load_configuration(path)?;
    if loaded.exists {
        return Ok(loaded);
    }

    let serialized = toml::to_string_pretty(&loaded.config).expect("defaults always serialize");
    if let Some(parent) = loaded.path.parent() {
        fs::create_dir_all(parent).map_err(|source| AppError::WriteConfig {
            path: loaded.path.clone(),
            source,
        })?;
    }
    fs::write(&loaded.path, serialized).map_err(|source| AppError::WriteConfig {
        path: loaded.path.clone(),
        source,
    })?;

    Ok(LoadedConfiguration {
        exists: true,
        ..loaded
    })
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
    speed: Option<f32>,
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
            // Speed is a user pace preference, not tied to a model choice.
            speed: self.speed.or(defaults.speed),
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

        // Defaults stay paired: no [tts] section keeps grok-voice + its voice.
        let defaults =
            parse_configuration("", Path::new("config.toml")).expect("empty config parses");
        assert_eq!(defaults.tts.model, "x-ai/grok-voice-tts-1.0");
        assert_eq!(defaults.tts.voice.as_deref(), Some("eve"));

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
    fn parses_tts_speed_and_keeps_it_optional() {
        let config = parse_configuration(
            r#"
            [tts]
            speed = 1.3
            "#,
            Path::new("config.toml"),
        )
        .expect("tts speed should parse");
        assert_eq!(config.tts.speed, Some(1.3));

        let defaults =
            parse_configuration("", Path::new("config.toml")).expect("empty config parses");
        assert_eq!(defaults.tts.speed, None);
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

    fn no_file(_path: &str) -> std::io::Result<String> {
        panic!("the key file must not be consulted in this case");
    }

    #[test]
    fn env_key_wins_over_file_and_keyring() {
        let key = super::resolve_api_key_from(
            Some("sk-env".to_owned()),
            Some("/run/agenix/openrouter-api-key".to_owned()),
            no_file,
            || panic!("the keyring must not be consulted in this case"),
        );
        assert_eq!(key.as_deref(), Some("sk-env"));
    }

    #[test]
    fn blank_env_key_falls_through_to_the_file() {
        let key = super::resolve_api_key_from(
            Some("   ".to_owned()),
            Some("/keys/openrouter".to_owned()),
            |path| {
                assert_eq!(path, "/keys/openrouter");
                Ok("  sk-file\n".to_owned())
            },
            || panic!("the keyring must not be consulted in this case"),
        );
        assert_eq!(key.as_deref(), Some("sk-file"), "file content is trimmed");
    }

    #[test]
    fn empty_key_file_falls_through_to_the_keyring() {
        let key = super::resolve_api_key_from(
            None,
            Some("/keys/openrouter".to_owned()),
            |_| Ok("\n".to_owned()),
            || Some("sk-keyring".to_owned()),
        );
        assert_eq!(key.as_deref(), Some("sk-keyring"));
    }

    #[test]
    fn unreadable_key_file_falls_through_to_the_keyring() {
        let key = super::resolve_api_key_from(
            None,
            Some("/keys/openrouter".to_owned()),
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            || Some("sk-keyring".to_owned()),
        );
        assert_eq!(key.as_deref(), Some("sk-keyring"));
    }

    #[test]
    fn no_source_at_all_yields_none() {
        let key = super::resolve_api_key_from(None, None, no_file, || None);
        assert_eq!(key, None);
    }
}
