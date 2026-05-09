{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_120 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.120";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-Dkkx8Bhfk+y/jEz9Fzwytmv2N3Gj/7ST+5MlPRzzetU=";
        };

        cargoHash = "sha256-5Zu/Sh9aBMxB+KGC1MHWJAQ8PuE40M6lsenkpFEwJ6A=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
