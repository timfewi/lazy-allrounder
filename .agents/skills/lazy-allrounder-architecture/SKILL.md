---
name: lazy-allrounder-architecture
description: Use this skill when implementing, reviewing, or refactoring code in lazy-allrounder so crate boundaries stay clear, platform/runtime logic stays isolated, and new features do not leak CLI, OS, or provider concerns across layers.
---

# lazy-allrounder architecture

Use this skill whenever a change touches more than one crate, adds a new workflow/runtime capability, or risks blurring the boundaries between `core`, `platform`, `integrations`, `app`, and `cli`.

This skill is the agent-side execution guardrail for the repository architecture. The human-facing source of truth is:

- `docs/adr.md`

## Purpose and when to use

Use this skill for:

- adding a new platform capability such as insertion, playback, hotkeys, or notifications
- adding a new workflow mode or changing workflow orchestration
- refactoring code that currently spans multiple crates
- reviewing whether a change belongs in `core`, `platform`, `integrations`, `app`, or `cli`

Do not use this skill only for tiny local edits that clearly stay within one existing module and do not affect repo boundaries.

## Required architecture rules

### 1. Respect crate ownership

- `core` owns product rules and workflow contracts
- `platform` owns OS/runtime behavior
- `integrations` owns provider APIs and wire formats
- `app` owns orchestration and dependency wiring
- `cli` owns parsing and terminal output

If a change touches more than one of these concerns, the join point should normally be `app`.

### 2. Keep `core` pure

`core` must not import:

- OS/process APIs
- HTTP clients
- provider payload types
- CLI parsing/output frameworks

If domain code starts needing those details, the boundary is wrong.

### 3. Keep provider logic out of `platform`

`platform` may manage microphone capture, pid/state files, insertion, playback, hotkeys, and notifications.

`platform` should not know about:

- OpenRouter
- HTTP requests
- auth headers
- model selection

If a feature needs both runtime mechanics and STT/TTS/text generation, let `app` orchestrate them.

### 4. Keep CLI thin

`cli` should:

- parse commands
- validate user-facing combinations
- print output
- call into `app`

`cli` should not manage runtime files, start OS processes directly, or construct provider clients.

### 5. Keep platform runtime code split

When platform runtime logic grows, keep it separated by reason to change.

Current Linux dictate runtime pattern:

- `dictate_runtime.rs` — public runtime API/types
- `platform.rs` — runtime orchestration
- `process_control.rs` — process/signal helpers
- `runtime_state.rs` — state/pid/lock/audio-file persistence

Do not collapse this back into one large file.

## Decision checklist before coding

1. What part is domain policy?
2. What part is OS/runtime mechanics?
3. What part is provider-specific?
4. Where do those pieces need to meet?
5. Is the meeting point `app`?

If the answer to 5 is "no", justify why the boundary is different.

## Common routing patterns

### New platform feature

Example: focused-app insertion

- runtime mechanics in `platform`
- workflow policy in `core` or `app`
- provider usage stays in `integrations`
- orchestration in `app`
- flags/printing in `cli`

### New generated workflow mode

Example: `teach` or `solve`

- request validation and mode contract in `core`
- provider calls in `integrations`
- orchestration in `app`
- command exposure in `cli`

Do not hide the mode inside `platform`.

### New long-running runtime behavior

Example: hotkey-triggered capture or playback control

- state files, pid tracking, locking in `platform`
- keep runtime modules small and explicit
- add focused state-transition tests before layering more behavior

## Red flags

Treat these as architecture smells:

- `cli` reaches into OS process handling
- `platform` imports provider clients
- `integrations` imports platform modules
- `app` becomes a dump for unrelated helpers instead of orchestration
- one platform file keeps growing because “it is easier for now”
- new config keys are added for features that do not have a real runtime implementation yet

## Validation

Before finishing a change that used this skill:

1. Re-read the touched public APIs only; ownership and responsibility should still be obvious.
2. Check that each touched crate still has one main reason to change.
3. Run the repo validation already used here:
   - `cargo test --offline`
   - `nix flake check`

## References

- `docs/adr.md`
- `README.md`
