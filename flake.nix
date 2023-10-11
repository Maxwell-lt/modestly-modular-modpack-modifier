{
  description = "modestly-modular-modpack-modifier";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
	pkgs = import nixpkgs {
	  inherit system overlays;
	};
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "llvm-tools-preview" ];
        };
	rustPlatform = pkgs.makeRustPlatform {
	  rustc = rust;
	  cargo = rust;
	};
      in with pkgs; {
        devShell = mkShell {
	  buildInputs = [
	    rust
            diffutils
            rust-analyzer
            cargo-nextest
            grcov
            cargo-llvm-cov
	  ];

	  shellHook = ''
	    export RUST_BACKTRACE=1
	  '';
	};

	defaultPackage = rustPlatform.buildRustPackage {
	  pname = "modestly-modular-modpack-modifier";
	  version = "0.5.1";

	  src = builtins.filterSource
	    (path: type: type != "symlink" && baseNameOf path != "target")
            ./.;

          # Skip tests that require internet access.
          checkFlags = [
            "--skip=modrinth::"
            "--skip=curse::"
            "--skip=di::orch::tests::test_orchestrator"
            "--skip=node::archive_downloader::tests::test_archive_downloader"
            "--skip=node::mod_resolver::tests::test_mod_resolver"
          ];

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = {
            mainProgram = "modestly-modular-modpack-modifier-cli";
          };
	};
  });
}
