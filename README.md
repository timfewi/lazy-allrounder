# lazy-allrounder

Cross-platform voice AI in Rust for dictation, speech-to-text, text-to-speech, and reading workflows on Windows, macOS, and Linux — with a floating overlay GUI.

## Description

`lazy-allrounder` combines the ideas behind `whisper-nix` and `lazy-reader-nix` into a single OS-agnostic application for Windows, macOS, and Linux.

It ships two front-ends over the same core:

- **`lazy-allrounder-gui`** — a small always-on-top badge pinned to a screen corner. Global hotkeys trigger read-aloud/summarize/explain/dictate on whatever you copied; the badge pulses while working and flashes green/red on completion. Clicking it opens a panel with the same modes as buttons, an ask-a-question box, and quit. Audio plays natively (no temp files).
- **`lazy-allrounder`** (CLI) — the same modes as composable commands (`dictate`, `read`, `explain`, `summarize`, `ask`), pipe-friendly.

## Installation

### Installers (easiest — no developer tools needed)

Tagged releases ship native installers built with cargo-packager on the [releases page](https://github.com/timfewi/lazy-allrounder/releases):

- **Windows**: `.exe` setup (NSIS)
- **macOS**: `.dmg` (drag to Applications)
- **Linux**: `.deb` (double-click in your software center) or AppImage (make executable and run)

On first launch the app asks for your OpenRouter API key right in its panel and stores it in the system keyring — no terminal, no config editing, and a "Start on login" toggle lives in the same panel. Unsigned builds: macOS Gatekeeper needs right-click → Open the first time; Windows SmartScreen needs "More info → Run anyway".

### Cargo (Windows, macOS, Linux)

```bash
cargo install --git https://github.com/timfewi/lazy-allrounder lazy-allrounder-gui lazy-allrounder-cli
```

Linux build dependencies: `pkg-config`, GTK3, ALSA, and DBus development headers (Debian/Ubuntu: `sudo apt install pkg-config libgtk-3-dev libayatana-appindicator3-dev libasound2-dev libdbus-1-dev libxkbcommon-dev`). macOS and Windows need no extra packages.

### Nix (Linux, macOS)

```bash
nix profile install github:timfewi/lazy-allrounder
```

Installs both binaries; `nix run github:timfewi/lazy-allrounder` starts the GUI directly.

### Release binaries

Tagged releases ship prebuilt archives for Linux (x86_64), macOS (arm64 + x86_64), and Windows (x86_64) on the [releases page](https://github.com/timfewi/lazy-allrounder/releases).

After installing, follow **Configuration** below (an OpenRouter API key + one config file), then run `lazy-allrounder-gui`.

## GUI

- The badge sits in the corner configured under `[overlay]` (default bottom-right).
- Global hotkeys (X11/macOS/Windows; defaults under `[hotkeys]`): `Super+S` read clipboard aloud, `Super+W` summarize, `Super+A` explain, `Super+Shift+A` ask, `Super+D` toggle dictation. Pressing a hotkey while audio plays stops it.
- Clicking the badge opens the panel: mode buttons, a question box for ask mode, a stop button while busy, and quit.

### Wayland notes (GNOME etc.)

Wayland compositors do not let applications position themselves, stay always-on-top, or grab global hotkeys:

- The badge opens as a normal window — position it once by hand (GNOME: right-click title area → Always on Top also works via `Super+Space` menu).
- For hotkeys, add desktop-native shortcuts that call the CLI, e.g. GNOME Settings → Keyboard → Custom Shortcuts with a command like `sh -c 'wl-paste | lazy-allrounder summarize --stdin'`.
- `LAZY_ALLROUNDER_UI=tray lazy-allrounder-gui` runs a tray-icon mode instead (needs a StatusNotifierItem tray; on GNOME that means the AppIndicator extension).

## Roadmap

### Done

- [x] Create the Rust workspace and crate boundaries
- [x] Add hosted model configuration through TOML + environment variables
- [x] Implement OpenRouter-backed text generation, speech-to-text, and text-to-speech flows
- [x] Ship a real `dictate` path for audio file/stdin -> transcript
- [x] Add real Linux dictate lifecycle/runtime commands
- [x] Add Linux global hotkey setup around the shared dictate runtime
- [x] Prepare the repository for public open source use
- [x] Add microphone capture for live dictation
- [x] Add platform-native playback (rodio)
- [x] Add cross-platform hotkey adapters (X11/macOS/Windows via global-hotkey)
- [x] Floating badge + panel overlay GUI (eframe/egui)
- [x] CI + release packaging for Windows, macOS, and Linux

### Next

- [ ] Focused-app text insertion beyond Linux (macOS/Windows)
- [ ] "Read current selection" via synthesized copy keystroke
- [ ] Hotkey rebinding UI inside the panel (config file works today)
- [ ] Signed/notarized release artifacts

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
cargo test-workspace
cargo run -p lazy-allrounder-cli -- config-path
```

`cargo test` still works, but `cargo test-workspace` prints a compact per-crate summary and only expands full output when a section fails.

### Nix

```bash
nix develop
nix flake check
```

## Usage

Hosted transcription commands require either `--stdin`, `--file`, or in the Linux dictate path `--microphone`.

Examples:

```bash
cat sample.wav | cargo run -p lazy-allrounder-cli -- dictate --stdin
cargo run -p lazy-allrounder-cli -- dictate --file ./sample.wav --output transcript.txt
cargo run -p lazy-allrounder-cli -- dictate --microphone
cargo run -p lazy-allrounder-cli -- dictate start
cargo run -p lazy-allrounder-cli -- dictate status
cargo run -p lazy-allrounder-cli -- dictate stop --output transcript.txt
cargo run -p lazy-allrounder-cli -- dictate stop -o transcript.txt
cargo run -p lazy-allrounder-cli -- dictate toggle
cargo run -p lazy-allrounder-cli -- dictate hotkey
cargo run -p lazy-allrounder-cli -- dictate hotkey --mode start
printf 'Explain this paragraph' | cargo run -p lazy-allrounder-cli -- explain --stdin
cargo run -p lazy-allrounder-cli -- summarize --file ./README.md
cargo run -p lazy-allrounder-cli -- ask --file ./README.md --question "What does this project do?"
```

`dictate` prints the transcript to stdout by default, or writes it to `--output`. On Linux, `dictate --microphone`, `dictate stop`, `dictate toggle`, and `dictate hotkey` now try to insert the transcript into the focused application when `--output` is not set. The primary path uses direct typing, and the fallback path stages the transcript on the clipboard and tries a paste shortcut. If insertion still fails, the CLI prints the transcript so it is not lost. `dictate --microphone` records from `pw-record` until you press Enter, then sends the captured WAV to OpenRouter STT. The Linux runtime commands use a visible state file at `$XDG_RUNTIME_DIR/lazy-allrounder-dictate.state` and currently report `idle`, `recording`, or `transcribing`. `dictate start`, `dictate status`, and `dictate hotkey --mode start` do not need model credentials, but `dictate stop`, `dictate toggle`, `dictate hotkey` (when stopping), and the one-shot transcription paths still need the normal STT config and API key. The text-to-speech commands write audio to `lazy-allrounder-<command>.mp3` by default. Use `--output <path>` to choose another file.

## Linux hotkey setup

Use `dictate hotkey` as the global shortcut target when you want one key to start dictation on the first press and stop/transcribe on the second press:

```bash
/absolute/path/to/lazy-allrounder dictate hotkey
```

The command defaults to `--mode toggle`. If you want separate bindings, use `dictate hotkey --mode start` and `dictate hotkey --mode stop`.

Hotkey launchers should use the same binary path and config path every time. If you do not use the default config path, include the global flag before the subcommand:

```bash
/absolute/path/to/lazy-allrounder --config /absolute/path/to/config.toml dictate hotkey
```

Desktop launchers often do not inherit shell-only environment variables. If your `OPENROUTER_API_KEY` is only exported in an interactive shell, use a small wrapper script or desktop launcher that exports it before calling `lazy-allrounder`. The first hotkey press only starts recording, but the second press needs the normal STT config and API key in order to transcribe.

Linux dependencies for the hotkey path are the same as the existing dictate runtime:

- `pw-record` for microphone capture
- on Wayland, `wtype` for direct typing and `wl-copy` for clipboard fallback
- on X11, `xdotool` for direct typing and `xclip` for clipboard fallback

Suggested setup paths:

### GNOME

1. Open **Settings -> Keyboard -> Keyboard Shortcuts -> View and Customize Shortcuts -> Custom Shortcuts**.
2. Add a shortcut that runs `/absolute/path/to/lazy-allrounder dictate hotkey`.
3. Bind it to your preferred key, for example `Super+d`.

### KDE Plasma

1. Open **System Settings -> Keyboard -> Shortcuts**.
2. Add a custom shortcut or command that runs `/absolute/path/to/lazy-allrounder dictate hotkey`.
3. Bind the same command to your preferred key.

### Sway

Add a binding such as:

```ini
bindsym $mod+d exec /absolute/path/to/lazy-allrounder dictate hotkey
```

### Hyprland

Add a binding such as:

```ini
bind = SUPER, D, exec, /absolute/path/to/lazy-allrounder dictate hotkey
```

### Generic X11 fallback

If your desktop environment does not expose global shortcuts cleanly, bind the same command in a shortcut daemon such as `sxhkd`:

```text
super + d
    /absolute/path/to/lazy-allrounder dictate hotkey
```

All of these setups reuse the same Linux dictate runtime state as `dictate start`, `dictate stop`, `dictate toggle`, and `dictate status`, so CLI-driven and hotkey-driven capture stay in sync.

## Security and public repo hygiene

- Do not commit API keys, personal config files, generated audio, or provider responses containing sensitive content.
- Use environment variables for secrets such as `OPENROUTER_API_KEY`.
- Report security issues privately; see [`SECURITY.md`](./SECURITY.md).

## Contributing

Please read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening a pull request.

## License

MIT. See [`LICENSE`](./LICENSE).
