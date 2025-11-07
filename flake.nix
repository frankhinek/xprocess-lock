{
  description = "Rust 1.89.0 + nightly rustfmt";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Stable compiler pinned at 1.89.0.
        # Use the *minimal* profile so we don't pull in stable rustfmt/clippy.
        rust-stable = pkgs.rust-bin.stable."1.89.0".minimal.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
          ];
        };

        # Pick the latest nightly that *has* rustfmt available.
        nightly-toolchain = pkgs.rust-bin.selectLatestNightlyWith (
          toolchain: toolchain.minimal.override { extensions = [ "rustfmt" ]; }
        );

        rustfmt-nightly = pkgs.symlinkJoin {
          name = "rustfmt-nightly";
          paths = [ nightly-toolchain ];
          postBuild = ''
            shopt -s nullglob
            for bin in "$out/bin/"*; do
              case "$(basename "$bin")" in
                rustfmt|cargo-fmt) ;;
                *) rm "$bin" ;;
              esac
            done
          '';
        };

      in
      {
        # Formatter used by `nix fmt`
        formatter = pkgs.nixfmt-rfc-style;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo-deny
            cargo-nextest
            just
            pkg-config
            rustfmt-nightly
            rust-stable
            sqlite
            taplo
          ];

          # Let IDEs and rust-analyzer find std sources
          RUST_SRC_PATH = "${rust-stable}/lib/rustlib/src/rust/library";
        };
      }
    );
}
