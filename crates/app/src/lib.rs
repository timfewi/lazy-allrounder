use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use lazy_allrounder_core::config::{AppConfiguration, ProviderConfiguration};
use lazy_allrounder_core::error::CoreError;
use lazy_allrounder_core::services::{
    AskRequest, GeneratedAudio, ReadRequest, ReadService, TransformService,
};
use lazy_allrounder_integrations::{
    OpenRouterClient, OpenRouterSpeechToTextClient, OpenRouterTextClient,
    OpenRouterTextToSpeechClient,
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

        Ok(Self {
            config,
            stt_client: OpenRouterSpeechToTextClient::new(shared_client.clone()),
            text_client: OpenRouterTextClient::new(shared_client.clone()),
            tts_client: OpenRouterTextToSpeechClient::new(shared_client),
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

    pub async fn read(&self, text: String) -> Result<Vec<u8>, AppError> {
        let service = ReadService::new(self.tts_client.clone());
        let request = ReadRequest::new(text)?;

        service
            .run(request, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn explain(&self, text: String) -> Result<GeneratedAudio, AppError> {
        let service = TransformService::new(self.text_client.clone(), self.tts_client.clone());
        let request = ReadRequest::new(text)?;

        service
            .explain(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn summarize(&self, text: String) -> Result<GeneratedAudio, AppError> {
        let service = TransformService::new(self.text_client.clone(), self.tts_client.clone());
        let request = ReadRequest::new(text)?;

        service
            .summarize(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }

    pub async fn ask(&self, text: String, question: String) -> Result<GeneratedAudio, AppError> {
        let service = TransformService::new(self.text_client.clone(), self.tts_client.clone());
        let request = AskRequest::new(text, question)?;

        service
            .ask(request, &self.config.text.model, &self.config.tts.model)
            .await
            .map_err(AppError::from)
    }
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
}

impl RawProviderConfiguration {
    fn merge_with_defaults(self, defaults: ProviderConfiguration) -> ProviderConfiguration {
        ProviderConfiguration {
            provider: self.provider.unwrap_or(defaults.provider),
            model: self.model.unwrap_or(defaults.model),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAppConfiguration {
    text: Option<RawProviderConfiguration>,
    stt: Option<RawProviderConfiguration>,
    tts: Option<RawProviderConfiguration>,
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
            [hotkeys]
            read = "Super+S"
            "#,
            Path::new("config.toml"),
        )
        .expect_err("unknown keys should fail");

        assert!(matches!(error, super::AppError::ParseConfig { .. }));
    }
}
