# lazy-allrounder

Cross-platform voice AI in Rust for dictation, speech-to-text, text-to-speech, and reading workflows on Windows, macOS, and Linux.

## Description

`lazy-allrounder` combines the ideas behind `whisper-nix` and `lazy-reader-nix` into a single OS-agnostic CLI-first application for Windows, macOS, and Linux.

The project currently supports hosted CLI workflows. Dictation is available as a real audio-file-or-stdin transcription flow, and Linux now has an early microphone capture path for direct dictation from the terminal. Hotkeys and native playback are intentionally not exposed until the platform adapters are real.

## Project status

`lazy-allrounder` is currently in an early CLI-first stage. The hosted model path is working, the repository is public, and the remaining work is mostly around turning the current command set into a true desktop-style cross-platform experience.

## Supported today

- `dictate`
- `read`
- `explain`
- `summarize`
- `ask`

## Roadmap

### Done

- [x] Create the Rust workspace and crate boundaries
- [x] Add hosted model configuration through TOML + environment variables
- [x] Implement OpenRouter-backed text generation, speech-to-text, and text-to-speech flows
- [x] Ship a real `dictate` path for audio file/stdin -> transcript
- [x] Prepare the repository for public open source use

### Current focus

- [ ] Keep the CLI workflows stable and sharpen the project messaging/docs

### Next

- [x] Add microphone capture for live dictation
- [ ] Add focused-app text insertion
- [ ] Add platform-native playback
- [ ] Add hotkeys only after real platform adapters exist
- [ ] Package and test releases for Windows, macOS, and Linux

## Configuration and opt-in

Copy [`config.example.toml`](./config.example.toml) to your config path and edit it before running hosted commands.

The default config path is OS-specific:

- Linux: `~/.config/lazy-allrounder/config.toml`
- macOS: `~/Library/Application Support/lazy-allrounder/config.toml`
- Windows: `%AppData%/lazy-allrounder/config.toml`

Current model defaults in the example config:

- Text generation: OpenRouter / `qwen/qwen3.6-flash`
- Speech-to-text: OpenRouter / `openai/whisper-large-v3-turbo`
- Text-to-speech: OpenRouter / `google/gemini-3.1-flash-tts-preview`

Secrets stay in environment variables, not in committed files:

```bash
export OPENROUTER_API_KEY="..."
```

## Development

### Rust

```bash
cargo test
cargo run -p lazy-allrounder-cli -- config-path
```

### Nix

```bash
nix develop
nix flake check
```

## Usage

All hosted commands require either `--stdin`, `--file`, or in the Linux dictate path `--microphone`.

Examples:

```bash
cat sample.wav | cargo run -p lazy-allrounder-cli -- dictate --stdin
cargo run -p lazy-allrounder-cli -- dictate --file ./sample.wav --output transcript.txt
cargo run -p lazy-allrounder-cli -- dictate --microphone
printf 'Explain this paragraph' | cargo run -p lazy-allrounder-cli -- explain --stdin
cargo run -p lazy-allrounder-cli -- summarize --file ./README.md
cargo run -p lazy-allrounder-cli -- ask --file ./README.md --question "What does this project do?"
```

`dictate` prints the transcript to stdout by default, or writes it to `--output`. On Linux, `dictate --microphone` records from `pw-record` until you press Enter, then sends the captured WAV to OpenRouter STT. The text-to-speech commands write audio to `lazy-allrounder-<command>.mp3` by default. Use `--output <path>` to choose another file.

## Security and public repo hygiene

- Do not commit API keys, personal config files, generated audio, or provider responses containing sensitive content.
- Use environment variables for secrets such as `OPENROUTER_API_KEY`.
- Report security issues privately; see [`SECURITY.md`](./SECURITY.md).

## Contributing

Please read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening a pull request.

## License

MIT. See [`LICENSE`](./LICENSE).
