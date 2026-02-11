{ pkgs, deciderModule }:
let
  runTest =
    module:
    pkgs.testers.runNixOSTest {
      imports = [ module ];
      defaults = {
        imports = [
          deciderModule
          ./common-vm.nix
        ];
      };
      globalTimeout = 3 * 60;
    };
in
pkgs.lib.mapAttrs' (
  name: _: pkgs.lib.nameValuePair (pkgs.lib.removeSuffix ".nix" name) (runTest (./tests + "/${name}"))
) (builtins.readDir ./tests)
