{
  description = "manzil — minimalist replacement for Home Manager's home.files";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs, ... }:
    let
      forSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
      pkgsFor    = system: nixpkgs.legacyPackages.${system};
    in {
      nixosModules.default = ./module.nix;
      nixosModules.manzil  = ./module.nix;

      packages = forSystems (system: rec {
        manzil  = (pkgsFor system).callPackage ./package.nix { };
        default = manzil;
      });
    };
}
