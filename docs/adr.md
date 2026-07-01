# lazy-allrounder architecture ADR

## Status

Accepted

## Context

`lazy-allrounder` is meant to become one cross-platform product, not a Linux-only script port. That makes architectural discipline more important than raw feature speed.

The current crate split is:

- `core`
- `platform`
- `integrations`
- `app`
- `cli`

That split only stays useful if contributors and agents keep each layer focused. Without explicit rules, feature work will slowly leak OS process logic, provider wire formats, and CLI concerns across the codebase until the boundaries stop being real.

The first concrete place this showed up was the Linux dictate runtime: process control, pid/state reconciliation, and lock handling are all necessary, but if they pile into one file they quickly become hard to read and hard to extend.

## Decision

The repository keeps a **five-layer architecture** with explicit ownership, and platform runtime code must stay split by responsibility.

### `core`

Owns product rules and workflow contracts.

- request validation
- shared workflow/domain types
- provider-agnostic service traits and rules
- domain errors

`core` must not import:

- OS/process APIs
- HTTP clients
- Clap or terminal IO
- concrete provider payloads

### `platform`

Owns OS-dependent capabilities.

- microphone capture
- focused-app insertion
- clipboard access
- playback
- hotkeys
- notifications
- runtime process/state coordination

`platform` must not own:

- provider request/response shaping
- workflow policy
- terminal UX

### `integrations`

Owns hosted-provider implementations.

- OpenRouter HTTP clients
- auth/header handling
- request/response serialization
- provider-specific error mapping

`integrations` must not own:

- desktop behavior
- hotkeys
- file-system runtime coordination
- CLI parsing

### `app`

Owns orchestration and dependency wiring.

- config loading
- provider selection
- platform/integration composition
- workflow handoff between layers
- app-facing error mapping

`app` is where platform and integrations meet. It should stay readable and orchestration-focused, not become a second platform layer or a second CLI layer.

### `cli`

Owns terminal-facing behavior only.

- parse commands and flags
- load config path inputs
- print output and user-facing messages
- forward work into `app`

`cli` must not directly manage:

- provider clients
- OS runtime files
- process control logic

## Platform runtime rule

Platform runtime code must stay split by **reason to change**, not by convenience.

For the Linux dictate runtime, the baseline structure is:

- `dictate_runtime.rs` — public API and user-facing runtime types
- `platform.rs` — Linux runtime orchestration
- `process_control.rs` — process launching, signal handling, and liveness checks
- `runtime_state.rs` — pid/state/lock/audio file persistence and stale-runtime recovery

Do not collapse platform runtime behavior back into one large file.

## Rules

1. Keep modules split by **reason to change**, not by convenience.
2. When a feature needs both platform behavior and provider behavior, join them in `app`, not in `platform` or `integrations`.
3. Keep one-shot workflow UX and long-running runtime/process coordination separate when they change for different reasons.
4. Platform runtime code must stay split into small modules rather than one large file.
5. If a boundary becomes unclear, prefer moving code outward:
   - policy -> `core`
   - OS/runtime mechanics -> `platform`
   - provider mechanics -> `integrations`
   - composition -> `app`
   - parsing/output -> `cli`
6. Do not add new config surface for features that do not yet have a real runtime implementation.
7. Add focused tests around state transitions and boundary behavior before layering more platform features on top.
8. When a module gains a second independent reason to change, split it before adding the next feature.
9. Prefer small, explicit helpers over clever multi-purpose runtime utilities.

## Consequences

- The codebase stays cognitively manageable as Linux, macOS, and Windows support grow.
- Cross-platform work can reuse the same workflow contracts instead of rewriting from scratch per OS.
- Reviews can reject boundary leakage early instead of after multiple features depend on it.
- Platform runtime work remains understandable enough to extend for insertion, playback, and hotkeys.
