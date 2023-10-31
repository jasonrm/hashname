{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    {
      overlays.default = final: prev: {
        hashname = self.packages.${final.system}.hashname;
      };
    }
    // (flake-utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};

      package = {
        lib,
        fetchFromGitHub,
        rustPlatform,
      }:
        rustPlatform.buildRustPackage rec {
          pname = "hashname";
          version = "1.1.0";

          src = lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with lib; {
            description = "Rename files to their hash";
            homepage = "https://github.com/xxkfqz/hashname";
            license = licenses.wtfpl;
          };
        };
    in {
      devShells.default = pkgs.mkShell {
        packages = [
          pkgs.bashInteractive
          pkgs.cargo
          pkgs.rustfmt
        ];
      };
      packages = {
        hashname = pkgs.callPackage package {};
      };
      apps.default = {
        type = "app";
        program = "${self.packages.${system}.hashname}/bin/hashname";
      };
    }));
}
