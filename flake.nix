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
      self,
      nixpkgs,
      flake-utils,
      crane,
      rust-overlay,
      ...
    }:
    {
      # `services.kaze`; see nix/module.nix.
      nixosModules.default = import ./nix/module.nix self;
    }
    // flake-utils.lib.eachDefaultSystem (
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
            # The pnpm store differs slightly between platforms, so each
            # system has its own hash. After changing web/pnpm-lock.yaml, build
            # (or let CI build) on each and copy the hash from the error.
            hash =
              {
                aarch64-darwin = "sha256-sW3YTWQ49bRpWJ6zX3kdVdnwotpQtOPzTsDQ8yzsbGQ=";
                x86_64-linux = "sha256-6uz8z88SK41kmX5LoNThzHYzAWFY4N52VweXMCbqZ6s=";
                aarch64-linux = "sha256-6uz8z88SK41kmX5LoNThzHYzAWFY4N52VweXMCbqZ6s=";
              }
              .${system} or lib.fakeHash;
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

        apps.default = flake-utils.lib.mkApp { drv = kaze; } // {
          meta.description = "Light, self-hosted manga downloader";
        };

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
              # The sandbox has no system CA store; reqwest needs one to build a client.
              SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
            }
          );
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            just
            cargo-audit
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

        formatter = pkgs.nixfmt;
      }
    );
}
