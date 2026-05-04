# Contributing

Thanks for contributing to `lazy-allrounder`.

## Before opening a pull request

1. Keep secrets out of the repository. Use environment variables for provider credentials.
2. Keep changes aligned with the architecture in the workspace and plan.
3. Run:

```bash
cargo fmt --all
cargo test-workspace
nix flake check
```

Use `cargo test-workspace` for the structured workspace report. `cargo test` remains available when you need the raw harness output.

## Development guidelines

- Keep `core` provider-agnostic.
- Keep OS-specific behavior inside `platform`.
- Keep hosted API details inside `integrations`.
- Keep CLI parsing thin and push orchestration into `app`.

## Pull request expectations

- Explain the behavior change.
- Note any config or provider changes.
- Call out security-sensitive areas such as secrets, network calls, hotkeys, audio capture, or text insertion.
