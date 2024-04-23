{...}: {
  perSystem = {
    pkgs,
    config,
    ...
  }: {
    nci.projects.porkg = {
      path = ./.;
      export = true;
      drvConfig = {
        mkDerivation = {
          buildInputs = with pkgs; [llvmPackages.clangUseLLVM llvmPackages.bintools cargo-nextest cargo-tarpaulin mold];
        };
      };
    };
    nci.crates.porkg-linux = {};
  };
}
