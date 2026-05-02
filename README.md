# lazy-allrounder

Cross-platform voice workflows in Rust: dictate, read, explain, summarize, and ask with hosted models.

## Description

`lazy-allrounder` combines the ideas behind `whisper-nix` and `lazy-reader-nix` into a single OS-agnostic CLI-first application for Windows, macOS, and Linux.

The project currently supports hosted CLI workflows. Dictation is available as a real audio-file-or-stdin transcription flow. Hotkeys and native playback are intentionally not exposed until the platform adapters are real.

## Current workflows

- `dictate`
- `read`
- `explain`
- `summarize`
- `ask`

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

All hosted commands require either `--stdin` or `--file`.

Examples:

```bash
cat sample.wav | cargo run -p lazy-allrounder-cli -- dictate --stdin
cargo run -p lazy-allrounder-cli -- dictate --file ./sample.wav --output transcript.txt
printf 'Explain this paragraph' | cargo run -p lazy-allrounder-cli -- explain --stdin
cargo run -p lazy-allrounder-cli -- summarize --file ./README.md
cargo run -p lazy-allrounder-cli -- ask --file ./README.md --question "What does this project do?"
```

`dictate` prints the transcript to stdout by default, or writes it to `--output`. The text-to-speech commands write audio to `lazy-allrounder-<command>.mp3` by default. Use `--output <path>` to choose another file.

## Security and public repo hygiene

- Do not commit API keys, personal config files, generated audio, or provider responses containing sensitive content.
- Use environment variables for secrets such as `OPENROUTER_API_KEY`.
- Report security issues privately; see [`SECURITY.md`](./SECURITY.md).

## Contributing

Please read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening a pull request.

## License

MIT. See [`LICENSE`](./LICENSE).
