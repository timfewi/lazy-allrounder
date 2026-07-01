{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { flake-utils, nixpkgs, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # nixos-25.11's plain rustc (1.91) is older than the crates.io MSRV of
        # the GUI stack (eframe/egui require rustc 1.92+; this workspace's own
        # Cargo.toml already declares rust-version = "1.94") — pin a modern
        # toolchain explicitly instead of relying on it.
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        guiLibPath = pkgs.lib.makeLibraryPath [
          pkgs.libGL
          pkgs.libxkbcommon
          pkgs.wayland
          pkgs.xorg.libX11
          pkgs.xorg.libXcursor
          pkgs.xorg.libXrandr
          pkgs.xorg.libXi
          pkgs.fontconfig
          pkgs.alsa-lib
          pkgs.dbus
        ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        lazy-allrounder = rustPlatform.buildRustPackage {
          pname = "lazy-allrounder";
          version = "0.1.0";
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: _type: baseNameOf path != "target";
          };
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.makeWrapper
          ];
          buildInputs = [
            pkgs.gtk3
            pkgs.dbus
            pkgs.alsa-lib
            # tray-icon's menu library (muda) links libxdo on Linux, which
            # ships with xdotool.
            pkgs.xdotool
          ];

          # The GUI dlopens GL/windowing libraries at runtime instead of
          # linking them, so the wrapper has to put them on the search path.
          postFixup = ''
            wrapProgram $out/bin/lazy-allrounder-gui \
              --prefix LD_LIBRARY_PATH : ${guiLibPath}
          '';

          meta = {
            description = "Cross-platform voice AI overlay: dictation, read-aloud, summarize, explain";
            homepage = "https://github.com/timfewi/lazy-allrounder";
            license = pkgs.lib.licenses.mit;
            mainProgram = "lazy-allrounder-gui";
          };
        };
      in
      {
        packages.default = lazy-allrounder;
        packages.lazy-allrounder = lazy-allrounder;

        apps.default = {
          type = "app";
          program = "${lazy-allrounder}/bin/lazy-allrounder-gui";
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.clippy
            pkgs.nixfmt-rfc-style
            pkgs.pipewire
            pkgs.rust-analyzer
            pkgs.wl-clipboard
            pkgs.wtype
            pkgs.xclip
            pkgs.xdotool
            # GUI (crates/gui) build + runtime deps: tray-icon needs pkg-config
            # + dbus + GTK3 (its default Linux backend — glib/atk/gdk-pixbuf/
            # cairo/pango come along as gtk3's propagated build inputs), eframe/
            # egui need pkg-config + GL/windowing/font libs, rodio (via cpal)
            # needs alsa-lib.
            pkgs.pkg-config
            pkgs.dbus
            pkgs.gtk3
            pkgs.alsa-lib
            pkgs.libGL
            pkgs.libxkbcommon
            pkgs.wayland
            pkgs.xorg.libX11
            pkgs.xorg.libXcursor
            pkgs.xorg.libXrandr
            pkgs.xorg.libXi
            pkgs.fontconfig
          ];

          LD_LIBRARY_PATH = guiLibPath;
        };

        formatter = pkgs.nixfmt-rfc-style;
      });
}
