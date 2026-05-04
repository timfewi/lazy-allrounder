use std::{
    fs,
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use lazy_allrounder_app::{
    Application, DictateCaptureOutcome, DictateState, dictate_runtime_status,
    dictate_start_capture, dictate_stop_capture, dictate_toggle_capture, load_configuration,
};
use tracing_subscriber::{EnvFilter, fmt};

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(name = "lazy-allrounder")]
#[command(about = "Cross-platform voice and reading workflow assistant")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Dictate(DictateCommand),
    Read {
        #[command(flatten)]
        input: TextInput,
    },
    Explain {
        #[command(flatten)]
        input: TextInput,
    },
    Summarize {
        #[command(flatten)]
        input: TextInput,
    },
    Ask {
        #[command(flatten)]
        input: TextInput,
        #[arg(long)]
        question: String,
    },
    ConfigPath,
}

#[derive(Debug, clap::Args)]
struct DictateCommand {
    #[command(subcommand)]
    action: Option<DictateAction>,
    #[command(flatten)]
    input: BinaryInput,
    #[arg(long, value_enum, conflicts_with = "microphone")]
    format: Option<AudioFormat>,
    #[arg(long, short = 'o')]
    output: Option<PathBuf>,
}

impl DictateCommand {
    fn validate(&self) -> Result<(), io::Error> {
        let has_input_flags =
            self.input.microphone || self.input.stdin || self.input.file.is_some();

        let Some(_) = &self.action else {
            return Ok(());
        };

        if has_input_flags || self.format.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dictate lifecycle commands cannot be combined with --microphone, --stdin, --file, or --format",
            ));
        }

        if self.output.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "one-shot dictate output flags must be passed without a lifecycle subcommand",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Subcommand)]
enum DictateAction {
    Start,
    Stop {
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    Toggle {
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    Status,
}

#[derive(Debug, clap::Args)]
struct TextInput {
    #[arg(long, group = "input")]
    stdin: bool,
    #[arg(long, group = "input")]
    file: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct BinaryInput {
    #[arg(long, group = "input")]
    microphone: bool,
    #[arg(long, group = "input")]
    stdin: bool,
    #[arg(long, group = "input")]
    file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum AudioFormat {
    Wav,
    Mp3,
    Flac,
    M4a,
    Ogg,
    Webm,
    Aac,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Dictate(dictate) => {
            dictate.validate()?;

            match dictate.action {
                Some(DictateAction::Start) => {
                    dictate_start_capture()?;
                    println!("{}", DictateState::Recording.as_str());
                }
                Some(DictateAction::Status) => {
                    let status = dictate_runtime_status()?;
                    println!("{}", status.state.as_str());
                }
                Some(DictateAction::Stop { output }) => {
                    let pending = dictate_stop_capture()?;
                    let application = load_application(cli.config.as_deref())?;
                    let transcript = application.transcribe_pending_dictation(pending).await?;
                    write_dictate_output(output.as_deref(), &transcript)?;
                }
                Some(DictateAction::Toggle { output }) => match dictate_toggle_capture()? {
                    DictateCaptureOutcome::Started => {
                        println!("{}", DictateState::Recording.as_str());
                    }
                    DictateCaptureOutcome::Pending(pending) => {
                        let application = load_application(cli.config.as_deref())?;
                        let transcript = application.transcribe_pending_dictation(pending).await?;
                        write_dictate_output(output.as_deref(), &transcript)?;
                    }
                },
                None => {
                    let application = load_application(cli.config.as_deref())?;
                    let transcript = if dictate.input.microphone {
                        eprintln!("Recording from the Linux microphone. Press Enter to stop.");
                        application.dictate_from_microphone().await?
                    } else {
                        let audio = read_binary_input(&dictate.input)?;
                        application
                            .dictate(
                                audio,
                                audio_format(dictate.format, dictate.input.file.as_deref()),
                            )
                            .await?
                    };
                    write_dictate_output(dictate.output.as_deref(), &transcript)?;
                }
            }
        }
        Command::Read { input } => {
            let application = load_application(cli.config.as_deref())?;
            let text = read_text_input(&input)?;
            let audio = application.read(text).await?;
            let output_path = write_audio_output("read", input.output.as_deref(), &audio)?;
            println!("{}", output_path.display());
        }
        Command::Explain { input } => {
            let application = load_application(cli.config.as_deref())?;
            let text = read_text_input(&input)?;
            let generated = application.explain(text).await?;
            let (content, audio) = generated.into_parts();
            let output_path = write_audio_output("explain", input.output.as_deref(), &audio)?;
            println!("{content}\n\nAudio: {}", output_path.display());
        }
        Command::Summarize { input } => {
            let application = load_application(cli.config.as_deref())?;
            let text = read_text_input(&input)?;
            let generated = application.summarize(text).await?;
            let (content, audio) = generated.into_parts();
            let output_path = write_audio_output("summarize", input.output.as_deref(), &audio)?;
            println!("{content}\n\nAudio: {}", output_path.display());
        }
        Command::Ask { input, question } => {
            let application = load_application(cli.config.as_deref())?;
            let text = read_text_input(&input)?;
            let generated = application.ask(text, question).await?;
            let (content, audio) = generated.into_parts();
            let output_path = write_audio_output("ask", input.output.as_deref(), &audio)?;
            println!("{content}\n\nAudio: {}", output_path.display());
        }
        Command::ConfigPath => {
            println!(
                "{}",
                load_configuration(cli.config.as_deref())?.path.display()
            );
        }
    }

    Ok(())
}

fn load_application(config_path: Option<&Path>) -> Result<Application, Box<dyn std::error::Error>> {
    let loaded = load_configuration(config_path)?;
    Ok(Application::from_loaded_configuration(&loaded)?)
}

fn read_binary_input(input: &BinaryInput) -> Result<Vec<u8>, io::Error> {
    let audio = if input.stdin {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        buffer
    } else {
        match &input.file {
            Some(path) => fs::read(path)?,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "either --microphone, --stdin, or --file must be provided",
                ));
            }
        }
    };

    if audio.len() > MAX_AUDIO_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "audio input is too large",
        ));
    }

    Ok(audio)
}

fn write_dictate_output(output: Option<&Path>, transcript: &str) -> Result<(), io::Error> {
    if let Some(output) = output {
        write_text_output(output, transcript)?;
        println!("{}", output.display());
    } else {
        println!("{transcript}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, DictateAction};

    #[test]
    fn dictate_accepts_microphone_input() {
        let cli = Cli::try_parse_from(["lazy-allrounder", "dictate", "--microphone"])
            .expect("microphone input should parse");

        assert!(matches!(cli.command, super::Command::Dictate { .. }));
    }

    #[test]
    fn dictate_rejects_multiple_input_sources() {
        let error = Cli::try_parse_from([
            "lazy-allrounder",
            "dictate",
            "--microphone",
            "--file",
            "sample.wav",
        ])
        .expect_err("multiple input sources should fail");

        let message = error.to_string();
        assert!(message.contains("--microphone"));
        assert!(message.contains("--file"));
    }

    #[test]
    fn dictate_rejects_microphone_with_explicit_format() {
        let error = Cli::try_parse_from([
            "lazy-allrounder",
            "dictate",
            "--microphone",
            "--format",
            "wav",
        ])
        .expect_err("microphone capture should not accept a manual format override");

        let message = error.to_string();
        assert!(message.contains("--microphone"));
        assert!(message.contains("--format"));
    }

    #[test]
    fn dictate_start_subcommand_parses() {
        let cli = Cli::try_parse_from(["lazy-allrounder", "dictate", "start"])
            .expect("start should parse");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Start),
                ..
            })
        ));
    }

    #[test]
    fn dictate_stop_accepts_output_after_subcommand() {
        let cli = Cli::try_parse_from([
            "lazy-allrounder",
            "dictate",
            "stop",
            "--output",
            "transcript.txt",
        ])
        .expect("stop output should parse after the lifecycle subcommand");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Stop { output: Some(_) }),
                ..
            })
        ));
    }

    #[test]
    fn dictate_stop_accepts_short_output_after_subcommand() {
        let cli =
            Cli::try_parse_from(["lazy-allrounder", "dictate", "stop", "-o", "transcript.txt"])
                .expect("stop short output should parse after the lifecycle subcommand");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Stop { output: Some(_) }),
                ..
            })
        ));
    }
}

fn audio_format(format: Option<AudioFormat>, path: Option<&Path>) -> &'static str {
    match format {
        Some(AudioFormat::Wav) => "wav",
        Some(AudioFormat::Mp3) => "mp3",
        Some(AudioFormat::Flac) => "flac",
        Some(AudioFormat::M4a) => "m4a",
        Some(AudioFormat::Ogg) => "ogg",
        Some(AudioFormat::Webm) => "webm",
        Some(AudioFormat::Aac) => "aac",
        None => match path
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("mp3") => "mp3",
            Some("flac") => "flac",
            Some("m4a") => "m4a",
            Some("ogg") => "ogg",
            Some("webm") => "webm",
            Some("aac") => "aac",
            _ => "wav",
        },
    }
}

fn read_text_input(input: &TextInput) -> Result<String, io::Error> {
    if input.stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        return Ok(buffer);
    }

    match &input.file {
        Some(path) => fs::read_to_string(path),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "either --stdin or --file must be provided",
        )),
    }
}

fn write_audio_output(
    command_name: &str,
    output: Option<&Path>,
    audio: &[u8],
) -> Result<PathBuf, io::Error> {
    let path = match output {
        Some(path) => path.to_path_buf(),
        None => PathBuf::from(format!("lazy-allrounder-{command_name}.mp3")),
    };

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                io::Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "{} already exists; choose --output explicitly",
                        path.display()
                    ),
                )
            } else {
                error
            }
        })?;
    file.write_all(audio)?;
    Ok(path)
}

fn write_text_output(path: &Path, text: &str) -> Result<(), io::Error> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                io::Error::new(
                    ErrorKind::AlreadyExists,
                    format!("{} already exists; choose another --output", path.display()),
                )
            } else {
                error
            }
        })?;
    file.write_all(text.as_bytes())
}
