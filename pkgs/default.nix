{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_123 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.123";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-ymeAEYsr7OnupWYJWjSeVGvq3+s+zxSNkODbzY62rYs=";
        };

        cargoHash = "sha256-d7x6gtx5OqEE4MyT6yjYn/qtgjx7GroTpXJewnBV2dU=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
