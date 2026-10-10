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

        # Cargo sources plus the non-Rust files the build and tests read.
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./crates/kaze/migrations
            ./.sqlx
            # Source test fixtures.
            (lib.fileset.fileFilter (file: file.hasExt "html" || file.hasExt "json") ./crates)
          ];
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          # Check `sqlx::query!` against the committed .sqlx/ metadata, not a live database.
          SQLX_OFFLINE = "true";
          buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];
        };

        # Dependencies are built once and cached separately from our own code.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # The SvelteKit UI, built on its own so Rust-only changes don't rebuild it.
        webSrc = lib.fileset.toSource {
          root = ./web;
          fileset = lib.fileset.difference ./web (
            lib.fileset.unions [
              (lib.fileset.maybeMissing ./web/node_modules)
              (lib.fileset.maybeMissing ./web/build)
              (lib.fileset.maybeMissing ./web/.svelte-kit)
            ]
          );
        };
        webArgs = finalAttrs: {
          version = "0.1.0";
          src = webSrc;
          nativeBuildInputs = [
            pkgs.nodejs_22
            pkgs.pnpm
            pkgs.pnpmConfigHook
          ];
          pnpmDeps = pkgs.fetchPnpmDeps {
            # One fetch shared by the build and the check.
            pname = "kaze-web";
            inherit (finalAttrs) version src;
            fetcherVersion = 4;
            # Update after changing web/pnpm-lock.yaml: build, copy the hash from the error.
            hash = "sha256-SR8gZ69OR8lLul/DsKBimG49IN2yxZdeahKEPNzBxCo=";
          };
        };
        web = pkgs.stdenvNoCC.mkDerivation (
          finalAttrs:
          webArgs finalAttrs
          // {
            pname = "kaze-web";
            buildPhase = "pnpm build";
            installPhase = "cp -r build $out";
          }
        );

        kaze = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            pname = "kaze";
            cargoExtraArgs = "--package kaze";
            # rust-embed compiles web/build into the binary.
            preBuild = ''
              mkdir -p web
              cp -r ${web} web/build
            '';
            doCheck = false; # tests run in `checks.nextest`
            meta.mainProgram = "kaze";
          }
        );
      in
      {
        packages = {
          default = kaze;
          inherit web;
        };

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
          web-check = pkgs.stdenvNoCC.mkDerivation (
            finalAttrs:
            webArgs finalAttrs
            // {
              pname = "kaze-web-check";
              buildPhase = "pnpm check && pnpm exec prettier --check .";
              installPhase = "touch $out";
            }
          );
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
          # sqlx checks queries against this database at compile time (`just db`).
          shellHook = ''
            export DATABASE_URL="sqlite:$(git rev-parse --show-toplevel)/target/dev.db"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
