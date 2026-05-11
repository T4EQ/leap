{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_121 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.121";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-ZOMgFNOcGkO66Jz/Z83eoIu+DIzo3Z/vq6Z5g6BDY/w=";
        };

        cargoHash = "sha256-DPdCDPTAPBrbqLUqnCwQu1dePs9lGg85JCJOCIr9qjU=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
