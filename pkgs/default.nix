{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_122 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.122";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-vO4RSxi/sMWxmsEs3GuljdMfIRSu75A+Q+c5wgYToRU=";
        };

        cargoHash = "sha256-Inup6vvJSG5ghNyeDPyZbfZo4d0LsMG2OJfStoaeDBs=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
