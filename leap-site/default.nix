{ inputs, ... }:
{
  perSystem =
    { config, pkgs, ... }:
    {
      packages = {
        # The site is always cross compiled using trunk and wasm. That's why we can build it with the
        # host toolchain, which already has support for wasm. However, the same is not true for
        # the target toolchain (aarch64-unknown-linux-musl), so we need to use a nix cross-compilation
        # strategy there
        leap-site = pkgs.rustPlatform.buildRustPackage {
          pname = "leap-site";
          version = "0.1.0";
          src = inputs.self;

          cargoLock = {
            lockFile = ./../Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            trunk
            config.packages.wasm-bindgen-cli_0_2_122
            dart-sass
            lld
          ];

          buildPhase = "
              runHook preBuild
              pushd leap-site
              trunk build --release --offline
              popd
              runHook postBuild
            ";

          installPhase = "
              runHook preInstall
              pushd leap-site
              mkdir -p $out
              cp -r dist $out/
              popd
              runHook postInstall
            ";
        };
      };
    };
}
