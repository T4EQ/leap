{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_125 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.125";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-zRawtjxMOdTMX+mZaiNR3YYfTiZJhf9qj7kXSSeMxrc=";
        };

        cargoHash = "sha256-aZCfgR23Qb0Pn4Mm4ToMtuuRQqSJjXCR9li/VvP5CTM=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
