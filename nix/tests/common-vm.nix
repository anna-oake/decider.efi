{
  config,
  pkgs,
  lib,
  modulesPath,
  ...
}:
{
  imports = [
    "${modulesPath}/profiles/minimal.nix"
  ];

  options.deciderTest.choiceImage = {
    choiceType = lib.mkOption {
      type = lib.types.enum [
        "nixos-current"
        "entry_id"
      ];
      default = "nixos-current";
    };
    entryId = lib.mkOption {
      type = lib.types.str;
      default = "";
    };
  };

  config = {
    virtualisation.useBootLoader = true;
    virtualisation.useEFIBoot = true;
    virtualisation.cores = 4;
    virtualisation.graphics = false;

    boot.loader.timeout = 0;
    boot.loader.efi.canTouchEfiVariables = true;
    boot.loader.decider.enable = true;

    virtualisation.qemu.options = lib.mkAfter (
      let
        choiceImage = pkgs.callPackage ./choice-image.nix {
          choiceType = config.deciderTest.choiceImage.choiceType;
          entryId = config.deciderTest.choiceImage.entryId;
        };
      in
      [
        "-drive if=none,id=decider-choice,format=raw,file=${choiceImage},snapshot=on"
        "-device usb-ehci,id=decider-usb-bus"
        "-device usb-storage,bus=decider-usb-bus.0,drive=decider-choice,removable=on"
      ]
    );
  };
}
