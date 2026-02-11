{
  pkgs,
  deciderModule,
  lanzaboote ? null,
}:
let
  runTest =
    module:
    pkgs.testers.runNixOSTest {
      imports = [ module ];
      node.specialArgs = pkgs.lib.optionalAttrs (lanzaboote != null) { inherit lanzaboote; };
      defaults = {
        imports = [
          deciderModule
          ./common-vm.nix
        ];
      };
      globalTimeout = 10 * 60;
    };
in
pkgs.lib.mapAttrs' (
  name: _: pkgs.lib.nameValuePair (pkgs.lib.removeSuffix ".nix" name) (runTest (./tests + "/${name}"))
) (builtins.readDir ./tests)
