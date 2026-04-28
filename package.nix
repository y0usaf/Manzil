{ rustPlatform, lib }:
rustPlatform.buildRustPackage {
  pname   = "manzil";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Minimalist file linker for the manzil NixOS module";
    mainProgram  = "manzil";
    license      = lib.licenses.mit;
    platforms    = lib.platforms.linux;
  };
}
