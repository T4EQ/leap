{ inputs, ... }:
{
  perSystem =
    {
      config,
      pkgs,
      rustPlatformFor,
      ...
    }:
    {
      packages =
        let
          rustPlatform = rustPlatformFor pkgs;
        in
        {
          leap-provision-site = rustPlatform.buildRustPackage {
            pname = "leap-provision-site";
            version = "0.1.0";
            src = inputs.self;

            cargoLock = {
              lockFile = ./../Cargo.lock;

              outputHashes = {
                "quircs-0.10.3" = "sha256-EfqYIk94LBmQOyROOOtbT6N13CkNv/dqlUK5/svRyX4=";
              };
            };

            nativeBuildInputs = with pkgs; [
              trunk
              config.packages.wasm-bindgen-cli_0_2_130
              dart-sass
              lld
            ];

            buildPhase = "
              runHook preBuild
              pushd leap-provision-site
              trunk build --release --offline
              popd
              runHook postBuild
            ";

            checkPhase = "
              # No tests for this package
            ";

            installPhase = "
              runHook preInstall
              pushd leap-provision-site
              mkdir -p $out
              cp -r dist $out/
              popd
              runHook postInstall
            ";
          };
        };
    };
}
