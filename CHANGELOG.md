# Changelog

All notable changes to lazy-allrounder are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-07-12

First release.

### Added
- **Overlay GUI** (`lazy-allrounder-gui`) — cross-platform always-on-top
  waveform badge + expandable panel with global hotkeys and playback
  controls; draggable, hardened geometry, works on GNOME Wayland.
- **Voice actions** — read aloud, summarize, explain, and ask about the
  current selection or clipboard; selection-first reading.
- **Dictation** — real microphone capture (PipeWire `pw-record` on Linux),
  speech-to-text via OpenRouter, and focused-app text insertion;
  `dictate --microphone` on the CLI with full lifecycle commands.
- **CLI** (`lazy-allrounder`) — all actions scriptable without the GUI.
- **Control socket** — drive the overlay from keyboard shortcuts or scripts.
- **TTS speed control** (`speed` config field) and configurable
  provider/model/voice.
- **Desktop integration** — self-installed `.desktop` entry + hicolor icon
  on Linux, so the app shows its real name and waveform icon.
- **Secrets** — API keys via plain config or agenix-managed key files.
- **Packaging** — Nix flake (`nix profile install github:timfewi/lazy-allrounder`),
  plus native installers (deb, AppImage, dmg, NSIS) and prebuilt archives for
  Linux, macOS (x86_64 + aarch64), and Windows via the release workflow.

[Unreleased]: https://github.com/timfewi/lazy-allrounder/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/timfewi/lazy-allrounder/releases/tag/v0.1.0
