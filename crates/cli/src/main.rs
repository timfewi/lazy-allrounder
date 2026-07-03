use std::{
    fs,
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use lazy_allrounder_app::{
    Application, DictateCaptureOutcome, DictateState, GuiAction, GuiCommand, SendError,
    dictate_runtime_status, dictate_start_capture, dictate_stop_capture, dictate_toggle_capture,
    load_configuration, notify, send_gui_command,
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
    /// Control a running overlay GUI (desktop keyboard shortcuts call this).
    Gui {
        #[command(subcommand)]
        action: GuiCliAction,
    },
    ConfigPath,
}

/// Commands forwarded to the overlay over its control socket. `dictate`
/// falls back to the headless recorder when no GUI is running, so a desktop
/// shortcut keeps working either way.
#[derive(Debug, Clone, Copy, Subcommand)]
enum GuiCliAction {
    Toggle,
    Read,
    Summarize,
    Explain,
    Ask,
    Dictate,
    Stop,
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

        let Some(action) = &self.action else {
            return Ok(());
        };

        if let DictateAction::Hotkey(hotkey) = action {
            hotkey.validate()?;
        }

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
    Hotkey(HotkeyCommand),
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
struct HotkeyCommand {
    #[arg(long, value_enum, default_value_t = HotkeyMode::Toggle)]
    mode: HotkeyMode,
    #[arg(long, short = 'o')]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum HotkeyMode {
    Toggle,
    Start,
    Stop,
}

impl HotkeyCommand {
    fn validate(&self) -> Result<(), io::Error> {
        if self.mode == HotkeyMode::Start && self.output.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "hotkey start mode cannot be combined with --output",
            ));
        }

        Ok(())
    }
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
        Command::Dictate(dictate) => handle_dictate(dictate, cli.config.as_deref()).await?,
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
        Command::Gui { action } => handle_gui(action, cli.config.as_deref()).await?,
        Command::ConfigPath => {
            println!(
                "{}",
                load_configuration(cli.config.as_deref())?.path.display()
            );
        }
    }

    Ok(())
}

async fn handle_gui(
    action: GuiCliAction,
    config_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let command = match action {
        GuiCliAction::Toggle => GuiCommand::TogglePanel,
        GuiCliAction::Stop => GuiCommand::Stop,
        GuiCliAction::Read => GuiCommand::Trigger(GuiAction::Read),
        GuiCliAction::Summarize => GuiCommand::Trigger(GuiAction::Summarize),
        GuiCliAction::Explain => GuiCommand::Trigger(GuiAction::Explain),
        GuiCliAction::Ask => GuiCommand::Trigger(GuiAction::Ask),
        GuiCliAction::Dictate => GuiCommand::Trigger(GuiAction::Dictate),
    };

    match send_gui_command(command) {
        Ok(()) => Ok(()),
        Err(SendError::NotRunning) => match action {
            // Dictation works GUI-less: same engine, same runtime files —
            // reuse the exact hotkey-toggle path.
            GuiCliAction::Dictate => {
                handle_hotkey(
                    HotkeyCommand {
                        mode: HotkeyMode::Toggle,
                        output: None,
                    },
                    config_path,
                )
                .await
            }
            // A stop with nothing running is what the user wanted anyway.
            GuiCliAction::Stop => {
                eprintln!("nothing to stop: the overlay GUI is not running");
                Ok(())
            }
            _ => {
                // The likely caller is a desktop shortcut with no terminal;
                // the notification is the only visible feedback.
                notify(
                    "Lazy Allrounder",
                    "The overlay is not running — start it from the app grid \
                     (or: systemctl --user start lazy-allrounder-gui)",
                );
                Err("the overlay GUI is not running".into())
            }
        },
        Err(error) => Err(error.to_string().into()),
    }
}

async fn handle_dictate(
    dictate: DictateCommand,
    config_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    dictate.validate()?;

    match dictate.action {
        Some(DictateAction::Hotkey(hotkey)) => handle_hotkey(hotkey, config_path).await?,
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
            let application = load_application(config_path)?;
            finish_dictation_capture(
                &application,
                DictateCaptureOutcome::Pending(pending),
                output.as_deref(),
            )
            .await?;
        }
        Some(DictateAction::Toggle { output }) => {
            let capture = dictate_toggle_capture()?;

            match capture {
                DictateCaptureOutcome::Started => {
                    println!("{}", DictateState::Recording.as_str());
                }
                pending => {
                    let application = load_application(config_path)?;
                    finish_dictation_capture(&application, pending, output.as_deref()).await?;
                }
            }
        }
        None => {
            let application = load_application(config_path)?;
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

            if dictate.input.microphone {
                deliver_dictated_transcript(&application, dictate.output.as_deref(), &transcript)?;
            } else {
                write_dictate_output(dictate.output.as_deref(), &transcript)?;
            }
        }
    }

    Ok(())
}

/// The desktop-shortcut entry point for dictation. A shortcut-spawned
/// process has no terminal, so desktop notifications carry the feedback:
/// recording started, transcript delivered, or what went wrong.
async fn handle_hotkey(
    hotkey: HotkeyCommand,
    config_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = run_hotkey(&hotkey, config_path).await;
    if let Err(error) = &result {
        notify("Dictation failed", &error.to_string());
    }

    result
}

async fn run_hotkey(
    hotkey: &HotkeyCommand,
    config_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let capture = match hotkey.mode {
        HotkeyMode::Toggle => dictate_toggle_capture()?,
        HotkeyMode::Start => {
            dictate_start_capture()?;
            DictateCaptureOutcome::Started
        }
        HotkeyMode::Stop => DictateCaptureOutcome::Pending(dictate_stop_capture()?),
    };

    match capture {
        DictateCaptureOutcome::Started => {
            notify("Dictation", "Recording — press the shortcut again to stop.");
            println!("{}", DictateState::Recording.as_str());
        }
        pending => {
            let application = load_application(config_path)?;
            let wrote_to_file = hotkey.output.is_some();
            finish_dictation_capture(&application, pending, hotkey.output.as_deref()).await?;
            notify(
                "Dictation",
                if wrote_to_file {
                    "Transcript saved."
                } else {
                    "Transcript inserted."
                },
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
    let mut stdout = io::stdout();
    write_dictate_output_to(output, transcript, &mut stdout)
}

fn write_dictate_output_to(
    output: Option<&Path>,
    transcript: &str,
    stdout: &mut impl Write,
) -> Result<(), io::Error> {
    if let Some(output) = output {
        write_text_output(output, transcript)?;
        writeln!(stdout, "{}", output.display())?;
    } else {
        writeln!(stdout, "{transcript}")?;
    }

    Ok(())
}

fn deliver_dictated_transcript(
    application: &Application,
    output: Option<&Path>,
    transcript: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    deliver_dictated_transcript_with(
        output,
        transcript,
        || {
            application
                .insert_dictated_text(transcript)
                .map_err(|error| Box::new(error) as _)
        },
        &mut stdout,
        &mut stderr,
    )
}

async fn finish_dictation_capture(
    application: &Application,
    capture: DictateCaptureOutcome,
    output: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    match capture {
        DictateCaptureOutcome::Started => {
            println!("{}", DictateState::Recording.as_str());
            Ok(())
        }
        DictateCaptureOutcome::Pending(pending) => {
            let transcript = application.transcribe_pending_dictation(pending).await?;
            deliver_dictated_transcript(application, output, &transcript)
        }
    }
}

fn deliver_dictated_transcript_with<Insert, Stdout, Stderr>(
    output: Option<&Path>,
    transcript: &str,
    insert_text: Insert,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> Result<(), Box<dyn std::error::Error>>
where
    Insert: FnOnce() -> Result<(), Box<dyn std::error::Error>>,
    Stdout: Write,
    Stderr: Write,
{
    if let Some(output) = output {
        write_dictate_output_to(Some(output), transcript, stdout)?;
        return Ok(());
    }

    match insert_text() {
        Ok(()) => Ok(()),
        Err(error) => {
            writeln!(
                stderr,
                "Failed to insert the transcript into the focused application. Printing it to stdout instead."
            )?;
            writeln!(stdout, "{transcript}")?;
            Err(error)
        }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command, DictateAction, HotkeyMode, deliver_dictated_transcript_with};

    #[test]
    fn dictate_accepts_microphone_input() {
        let cli = Cli::try_parse_from(["lazy-allrounder", "dictate", "--microphone"])
            .expect("microphone input should parse");

        assert!(matches!(cli.command, super::Command::Dictate { .. }));
    }

    #[test]
    fn gui_subcommands_parse() {
        let toggle = Cli::try_parse_from(["lazy-allrounder", "gui", "toggle"])
            .expect("gui toggle should parse");
        assert!(matches!(
            toggle.command,
            Command::Gui {
                action: super::GuiCliAction::Toggle
            }
        ));

        let dictate = Cli::try_parse_from(["lazy-allrounder", "gui", "dictate"])
            .expect("gui dictate should parse");
        assert!(matches!(
            dictate.command,
            Command::Gui {
                action: super::GuiCliAction::Dictate
            }
        ));

        let stop =
            Cli::try_parse_from(["lazy-allrounder", "gui", "stop"]).expect("gui stop should parse");
        assert!(matches!(
            stop.command,
            Command::Gui {
                action: super::GuiCliAction::Stop
            }
        ));
    }

    #[test]
    fn gui_requires_an_action() {
        Cli::try_parse_from(["lazy-allrounder", "gui"]).expect_err("bare gui should fail");
        Cli::try_parse_from(["lazy-allrounder", "gui", "dance"])
            .expect_err("unknown gui action should fail");
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
    fn dictate_hotkey_defaults_to_toggle_mode() {
        let cli = Cli::try_parse_from(["lazy-allrounder", "dictate", "hotkey"])
            .expect("hotkey should parse");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Hotkey(super::HotkeyCommand {
                    mode: HotkeyMode::Toggle,
                    output: None,
                })),
                ..
            })
        ));
    }

    #[test]
    fn dictate_hotkey_accepts_explicit_start_mode() {
        let cli = Cli::try_parse_from(["lazy-allrounder", "dictate", "hotkey", "--mode", "start"])
            .expect("hotkey start mode should parse");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Hotkey(super::HotkeyCommand {
                    mode: HotkeyMode::Start,
                    ..
                })),
                ..
            })
        ));
    }

    #[test]
    fn dictate_hotkey_accepts_output_after_subcommand() {
        let cli = Cli::try_parse_from([
            "lazy-allrounder",
            "dictate",
            "hotkey",
            "--output",
            "transcript.txt",
        ])
        .expect("hotkey output should parse");

        assert!(matches!(
            cli.command,
            Command::Dictate(super::DictateCommand {
                action: Some(DictateAction::Hotkey(super::HotkeyCommand {
                    mode: HotkeyMode::Toggle,
                    output: Some(_),
                })),
                ..
            })
        ));
    }

    #[test]
    fn dictate_hotkey_rejects_output_in_start_mode() {
        let cli = Cli::try_parse_from([
            "lazy-allrounder",
            "dictate",
            "hotkey",
            "--mode",
            "start",
            "--output",
            "transcript.txt",
        ])
        .expect("clap should parse hotkey start mode before custom validation");

        let Command::Dictate(dictate) = cli.command else {
            panic!("expected dictate command");
        };

        let error = dictate
            .validate()
            .expect_err("hotkey start mode should reject output during command validation");

        assert!(
            error
                .to_string()
                .contains("hotkey start mode cannot be combined with --output")
        );
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

    #[test]
    fn dictate_delivery_preserves_transcript_when_insertion_fails() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let error = deliver_dictated_transcript_with(
            None,
            "final transcript",
            || Err(Box::new(std::io::Error::other("insert failed"))),
            &mut stdout,
            &mut stderr,
        )
        .expect_err("failed insertion should still return an error");

        assert_eq!(error.to_string(), "insert failed");
        assert_eq!(
            String::from_utf8(stdout).expect("stdout should be utf8"),
            "final transcript\n"
        );
        assert_eq!(
            String::from_utf8(stderr).expect("stderr should be utf8"),
            "Failed to insert the transcript into the focused application. Printing it to stdout instead.\n"
        );
    }

    #[test]
    fn dictate_delivery_skips_insertion_when_output_path_is_requested() {
        let temp_path =
            std::env::temp_dir().join(format!("lazy-allrounder-test-{}.txt", std::process::id()));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let output = PathBuf::from(&temp_path);

        deliver_dictated_transcript_with(
            Some(output.as_path()),
            "saved transcript",
            || panic!("insertion should not be attempted when --output is used"),
            &mut stdout,
            &mut stderr,
        )
        .expect("file output path should bypass insertion");

        assert_eq!(
            std::fs::read_to_string(&temp_path).expect("transcript file should exist"),
            "saved transcript"
        );
        assert_eq!(
            String::from_utf8(stdout).expect("stdout should be utf8"),
            format!("{}\n", temp_path.display())
        );
        assert!(stderr.is_empty());

        std::fs::remove_file(temp_path).expect("temporary transcript file should be removed");
    }
}
