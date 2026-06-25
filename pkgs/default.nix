{ ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages = {
        wasm-bindgen-cli_0_2_126 = pkgs.rustPlatform.buildRustPackage rec {
        pname = "wasm-bindgen-cli";
        version = "0.2.126";

        src = pkgs.fetchCrate {
          inherit pname version;
          hash = "sha256-H6Is3fiZVxZCfOMWK5dWMSrtn50VGv0sfdnsT+cTtyk=";
        };

        cargoHash = "sha256-VucqkXbCi4qtQzY/HrXiDnbSURsagPsdNVMn1Tw3UiY=";

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [ openssl ];

        # Tests require a compiled .wasm artifact; skip them.
        doCheck = false;
      };
    };
  };
}
