{
  description = "decider.efi integration tests";

  inputs = {
    decider.url = "path:../..";
    nixpkgs.follows = "decider/nixpkgs";
    lanzaboote = {
      url = "github:nix-community/lanzaboote/1902463415745b992dbaf301b2a35a1277be1584";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      decider,
      nixpkgs,
      lanzaboote,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      mkChecks =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          testChecks = pkgs.lib.mapAttrs' (name: value: pkgs.lib.nameValuePair "test-${name}" value) (
            import ./default.nix {
              inherit pkgs;
              deciderModule = decider.nixosModules.decider;
              inherit lanzaboote;
            }
          );
        in
        decider.checks.${system} // testChecks;
    in
    {
      checks = nixpkgs.lib.genAttrs systems mkChecks;
    };
}
