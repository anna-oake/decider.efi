{
  lib,
  config,
  pkgs,
  deciderPackage ? null,
  ...
}:
let
  cfg = config.boot.loader.decider;
  efi = config.boot.loader.efi;
  systemdBoot = config.boot.loader.systemd-boot;

  efiArch = pkgs.stdenv.hostPlatform.efiArch;
  deciderEfiName = "decider${efiArch}.efi";
  fallbackEfiName = "BOOT${lib.toUpper efiArch}.EFI";
  defaultChainloadPath = "/EFI/systemd/systemd-boot${efiArch}.efi";

  deciderConfig = pkgs.writeText "decider.conf" ''
    chainload_path=${cfg.chainloadPath}
    entries_path=${cfg.entriesPath}
  '';

  updateEfiBootEntry = pkgs.writeShellScript "decider-update-efi-boot-entry" ''
    set -euo pipefail

    label='decider.efi'
    esp_mount=${lib.escapeShellArg efi.efiSysMountPoint}
    loader_path='/EFI/decider/${deciderEfiName}'

    efibootmgr='${lib.getExe pkgs.efibootmgr}'
    findmnt='${pkgs.util-linux}/bin/findmnt'
    lsblk='${pkgs.util-linux}/bin/lsblk'
    readlink='${pkgs.coreutils}/bin/readlink'
    head='${pkgs.coreutils}/bin/head'

    part_source="$("$findmnt" -no SOURCE --target "$esp_mount")"
    part_device="$("$readlink" -f "$part_source")"
    disk_name="$("$lsblk" -no PKNAME "$part_device" | "$head" -n1)"
    disk_device="/dev/$disk_name"
    part_number="''${part_device#"$disk_device"}"
    part_number="''${part_number#p}"

    "$efibootmgr" -B -L "$label" >/dev/null 2>&1 || true

    "$efibootmgr" \
      -c \
      -d "$disk_device" \
      -p "$part_number" \
      -l "$loader_path" \
      -L "$label" >/dev/null
  '';
in
{
  options.boot.loader.decider = {
    enable = lib.mkEnableOption "install decider.efi as the primary systemd-boot chainloader";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = deciderPackage;
      defaultText = lib.literalExpression ''
        self.packages.${pkgs.stdenv.hostPlatform.system}.decider-efi
      '';
      description = ''
        Package that provides {file}`bin/decider.efi`.
      '';
    };

    chainloadPath = lib.mkOption {
      type = lib.types.str;
      default = defaultChainloadPath;
      defaultText = lib.literalExpression ''"\\EFI\\systemd\\systemd-boot''${pkgs.stdenv.hostPlatform.efiArch}.efi"'';
      description = ''
        UEFI path that decider will chainload after setting LoaderEntryOneShot.
      '';
    };

    entriesPath = lib.mkOption {
      type = lib.types.str;
      default = "/loader/entries";
      description = ''
        UEFI path to the systemd-boot entries directory used when `mode=nixos-current`.
      '';
    };

    efiInstallAsRemovable = lib.mkOption {
      type = lib.types.bool;
      default = !efi.canTouchEfiVariables;
      defaultText = lib.literalExpression "!config.boot.loader.efi.canTouchEfiVariables";
      description = ''
        Install decider at {file}`EFI/BOOT/${fallbackEfiName}` so firmware fallback
        boot still works without EFI variable updates.
      '';
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = systemdBoot.enable;
            message = "boot.loader.decider.enable requires boot.loader.systemd-boot.enable = true.";
          }
          {
            assertion = cfg.package != null;
            message = "boot.loader.decider.package must be set (or import the module from this flake).";
          }
          {
            assertion = systemdBoot.xbootldrMountPoint == null;
            message = "boot.loader.decider currently does not support boot.loader.systemd-boot.xbootldrMountPoint.";
          }
        ];
      }

      (lib.mkIf (cfg.package != null) {
        boot.loader.systemd-boot.extraFiles = lib.mkMerge [
          {
            "EFI/decider/${deciderEfiName}" = "${cfg.package}/bin/decider.efi";
            "decider/decider.conf" = deciderConfig;
          }
          (lib.mkIf cfg.efiInstallAsRemovable {
            "EFI/BOOT/${fallbackEfiName}" = "${cfg.package}/bin/decider.efi";
          })
        ];
      })

      (lib.mkIf (cfg.package != null && efi.canTouchEfiVariables) {
        boot.loader.systemd-boot.extraInstallCommands = lib.mkAfter ''
          ${updateEfiBootEntry}
        '';
      })
    ]
  );
}
