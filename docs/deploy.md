# Rebuild & Deploy — lazy-allrounder

(formerly "lazy-reader"; the GNOME launcher entry is `Lazy Allrounder`)

The app is installed on this machine via `nix profile` tracking
`github:timfewi/lazy-allrounder` (branch main). The GNOME desktop entry
launches `~/.nix-profile/bin/lazy-allrounder-gui`, which is stable across
upgrades — deploying never touches the desktop entry.

## Dev loop (fast iteration)

```bash
cd ~/code/lazy-allrounder
nix develop            # dev shell with rust toolchain + GUI libs on LD_LIBRARY_PATH
cargo run --bin lazy-allrounder-gui
```

**Warning:** running a `target/debug` build rewrites
`~/.local/share/applications/lazy-allrounder-gui.desktop` to point at the
debug binary (see `crates/platform/src/desktop_entry.rs`). That binary only
works inside the dev shell — the GNOME icon will silently fail afterwards
(`libxkbcommon-x11.so` panic in the journal). Fix: repoint `Exec=` back to
`/home/tim/.nix-profile/bin/lazy-allrounder-gui`, or just deploy (below) and
the profile path keeps working.

## Verify the release build locally (optional, no push needed)

```bash
nix build              # → ./result/bin/lazy-allrounder-gui (wrapped, works outside dev shell)
./result/bin/lazy-allrounder-gui
```

## Deploy

```bash
# 1. Land the change on GitHub main
git add -p && git commit && git push

# 2. Rebuild the installed package from GitHub
nix profile upgrade lazy-allrounder

# 3. Restart the running instance
pkill -f '[l]azy-allrounder-gui$'
gio launch ~/.local/share/applications/lazy-allrounder-gui.desktop   # or click the icon
```

Confirm the deployed rev:

```bash
nix profile list | grep -A3 lazy-allrounder   # Locked flake URL shows the commit
journalctl --user -S -5min | grep -i allrounder   # no panics/config errors
```

## Gotchas

- **Config vs binary skew:** new config fields (e.g. `speed`) are rejected by
  older binaries — the app then starts *unconfigured* instead of crashing.
  If the overlay comes up but TTS is dead, check the journal for
  `failed to parse configuration` and upgrade the profile.
- `nix profile upgrade` builds whatever is on GitHub main — unpushed local
  commits are not deployed. Use `nix build` + `./result/...` to test local
  state.
- The nix package ships no `.desktop` file; the per-user entry is the only
  launcher. Don't delete it.
