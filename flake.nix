{
  description = "Low-Bandwidth Educational Access Platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
    buildroot-nix.url = "github:velentr/buildroot.nix/master";
  };

  outputs =
    inputs@{ self, flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { lib, ... }:
      {
        imports = [
          ./leap-site
          ./leap-provision-site
          ./leap-server
          ./leap-linux
          ./pkgs
        ];

        options = {
          git-rev = lib.mkOption {
            type = lib.types.str;
            description = "The git revision of this reporsitory";
          };
        };

        config = {
          systems = [
            "aarch64-darwin"
            "aarch64-linux"
            "x86_64-linux"
          ];

          git-rev = "${self.rev or self.dirtyRev or "Unknown"}";

          perSystem =
            { config,pkgs, ... }:
            {
              packages.default = config.packages.leap;

              devShells = rec {
                leap =
                  let
                    pkgsWithRustOverlay = import inputs.nixpkgs {
                      inherit (pkgs.stdenv.hostPlatform) system;
                      overlays = [ inputs.rust-overlay.overlays.default ];
                    };
                  in
                  pkgsWithRustOverlay.mkShell {
                    nativeBuildInputs = with pkgsWithRustOverlay; [
                      (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
                      trunk
                      config.packages.wasm-bindgen-cli_0_2_122
                      dart-sass
                      cargo-watch
                      cargo-deny
                      diesel-cli
                      bunyan-rs
                    ];
                  };

                default = leap;
              };
            };
        };
      }
    );
}
