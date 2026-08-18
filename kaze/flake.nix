{
  description = "Kaze - Manga download manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        
        craneLib = crane.mkLib pkgs;
        src = pkgs.lib.cleanSource ./.;
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        buildInputs = with pkgs; [
          openssl
          sqlite
        ] ++ lib.optionals stdenv.isLinux [
          chromium
          nss
          atk
          at-spi2-atk
          cups
          dbus
          expat
          fontconfig
          freetype
          glib
          gtk3
          libdrm
          libxkbcommon
          mesa
          nspr
          pango
          xorg.libX11
          xorg.libXcomposite
          xorg.libXdamage
          xorg.libXext
          xorg.libXfixes
          xorg.libXrandr
        ];

        nativeBuildInputs = with pkgs; [ 
          pkg-config 
          rustfmt
        ];

        cargoArtifacts = craneLib.buildDepsOnly {
          inherit src buildInputs nativeBuildInputs;
          cargoExtraArgs = "--features chromium";
        };

        kaze = craneLib.buildPackage {
          inherit src buildInputs nativeBuildInputs cargoArtifacts;
          cargoExtraArgs = "--features chromium";
        };
      in
      {
        packages.default = kaze;
        
        apps.default = { 
          type = "app"; 
          program = "${kaze}/bin/kaze"; 
        };
        
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ [
            rustToolchain
            pkgs.cargo-watch
            pkgs.cargo-edit
            pkgs.sqlfluff
          ];
          
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          CARGO_TERM_COLOR = "always";
        };
      }
    );
}
