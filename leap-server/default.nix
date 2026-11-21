top@{ inputs, ... }:
{
  perSystem =
    {
      config,
      pkgs,
      rustPlatformFor,
      ...
    }:
    let
      leapPackageWithPkgs =
        pkgs: targetPkgs: opts:
        let
          rustPlatform = rustPlatformFor targetPkgs;
        in
        rustPlatform.buildRustPackage {
          pname = "leap-server";
          version = "0.1.0";
          src = inputs.self;

          cargoLock = {
            lockFile = ./../Cargo.lock;

            outputHashes = {
              "quircs-0.10.3" = "sha256-EfqYIk94LBmQOyROOOtbT6N13CkNv/dqlUK5/svRyX4=";
            };
          };

          nativeBuildInputs = with pkgs; [
            cacert # needed by the tests in this package.
            lld
          ];

          dontCargoUseSystemLibs = true;

          # Test the same feature set as the Nix binary, which gets its revision from Nix.
          checkNoDefaultFeatures = true;
          checkFeatures = [ "provision" ];

          buildPhase = ''
            runHook preBuild
            pushd leap-server
            export LEAP_SERVER_FRONTEND_PATH="${config.packages.leap-site}/dist"
            export LEAP_SERVER_PROVISIONING_PATH="${config.packages.leap-provision-site}/dist"
            export CARGO_TARGET_DIR="$(pwd)/../target"
            export LEAP_SERVER_NIX_GIT_REVISION="${top.config.git-rev}"
            # GCC compiles by default with -moutline-atomics, but this is not supported by the musl
            # variant of the build, as it would require linking to libgcc. This is needed by libdbus.
            export CFLAGS="$CFLAGS ${opts.extra-cflags or ""}"
            cargo build --release --offline -j $NIX_BUILD_CORES --target ${targetPkgs.stdenv.hostPlatform.rust.rustcTarget} --no-default-features --features provision
            popd
            runHook postBuild
          '';
        };

      targetPkgs = import inputs.nixpkgs {
        inherit (pkgs.stdenv.hostPlatform) system;
        crossSystem = {
          isStatic = true;
          config = "aarch64-unknown-linux-musl";
        };
      };
    in
    {
      packages = {
        leap = leapPackageWithPkgs pkgs pkgs { };
        leap-target = leapPackageWithPkgs pkgs targetPkgs { extra-cflags = "-mno-outline-atomics"; };
      };
    };
}
