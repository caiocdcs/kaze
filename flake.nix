{
  description = "kaze — light, self-hosted manga downloader";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      crane,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        inherit (pkgs) lib;

        # One toolchain for dev shell, builds and CI, pinned by rust-toolchain.toml + flake.lock.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];
        };

        # Dependencies are built once and cached separately from our own code.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        kaze = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            pname = "kaze";
            cargoExtraArgs = "--package kaze";
            doCheck = false; # tests run in `checks.nextest`
            meta.mainProgram = "kaze";
          }
        );
      in
      {
        packages.default = kaze;

        apps.default = flake-utils.lib.mkApp { drv = kaze; };

        checks = {
          inherit kaze;
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );
          fmt = craneLib.cargoFmt { inherit src; };
          nextest = craneLib.cargoNextest (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoNextestExtraArgs = "--no-tests=pass";
            }
          );
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            just
            cargo-nextest
            cargo-watch
            sqlx-cli
            sqlite
            nodejs_22
            pnpm
          ];
          RUST_LOG = "kaze=debug";
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
