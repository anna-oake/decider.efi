{
  lib,
  config,
  options,
  pkgs,
  deciderPackage,
  ...
}:
let
  cfg = config.boot.loader.decider;
  efi = config.boot.loader.efi;
  hasLanzabooteOption = options ? boot && options.boot ? lanzaboote;
  systemdBoot = config.boot.loader."systemd-boot";

  lanzaboote =
    if hasLanzabooteOption then
      config.boot.lanzaboote
    else
      {
        enable = false;
        allowUnsigned = false;
        publicKeyFile = "";
        privateKeyFile = "";
        installCommand = "";
        extraEfiSysMountPoints = [ ];
        autoEnrollKeys.enable = false;
      };

  systemdBootEnabled = systemdBoot.enable;
  lanzabooteEnabled = lanzaboote.enable;
  lanzabooteAllowUnsigned = lanzaboote.allowUnsigned;
  lanzabootePublicKeyFile = toString lanzaboote.publicKeyFile;
  lanzabootePrivateKeyFile = toString lanzaboote.privateKeyFile;
  lanzabooteEfiSysMountPoints = [ efi.efiSysMountPoint ] ++ lanzaboote.extraEfiSysMountPoints;

  efiArch = pkgs.stdenv.hostPlatform.efiArch;
  deciderEfiName = "decider${efiArch}.efi";
  fallbackEfiName = "BOOT${lib.toUpper efiArch}.EFI";
  deciderPackageEfiPath = "${cfg.package}/bin/decider.efi";
  defaultChainloadPath = "/EFI/systemd/systemd-boot${efiArch}.efi";

  deciderConfig = pkgs.writeText "decider.conf" ''
    chainload_path=${cfg.chainloadPath}
    choice_source=${cfg.choiceSource}
    ${lib.optionalString (cfg.choiceSource == "tftp") "tftp_ip=${cfg.tftpIp}"}
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

  installDeciderLanzaboote = pkgs.writeShellScript "decider-install-lanzaboote" ''
    set -euo pipefail

    esp_mount="''${1:?ESP mount point required}"
    src=${lib.escapeShellArg deciderPackageEfiPath}
    conf_src=${lib.escapeShellArg deciderConfig}
    decider_dst="$esp_mount/EFI/decider/${deciderEfiName}"
    fallback_dst="$esp_mount/EFI/BOOT/${fallbackEfiName}"

    allow_unsigned='${lib.boolToString lanzabooteAllowUnsigned}'
    public_key=${lib.escapeShellArg lanzabootePublicKeyFile}
    private_key=${lib.escapeShellArg lanzabootePrivateKeyFile}

    install='${pkgs.coreutils}/bin/install'
    mkdir='${pkgs.coreutils}/bin/mkdir'
    dirname='${pkgs.coreutils}/bin/dirname'
    sbsign='${lib.getExe' pkgs.sbsigntool "sbsign"}'

    if [ ! -r "$src" ]; then
      echo "decider: source EFI binary not found: $src" >&2
      exit 1
    fi

    echo "decider: installing via lanzaboote to ESP at $esp_mount" >&2

    install_signed_or_unsigned() {
      local dst="$1"
      "$mkdir" -p "$("$dirname" "$dst")"

      if [ "$allow_unsigned" = "true" ]; then
        if [ -r "$public_key" ] && [ -r "$private_key" ]; then
          if "$sbsign" --key "$private_key" --cert "$public_key" --output "$dst" "$src" >/dev/null 2>&1; then
            return 0
          fi
        fi
        "$install" -m 0644 "$src" "$dst"
        return 0
      fi

      if [ ! -r "$public_key" ] || [ ! -r "$private_key" ]; then
        echo "decider: secure boot keys are missing, cannot sign decider.efi (set boot.lanzaboote.allowUnsigned = true to allow unsigned install)" >&2
        return 1
      fi

      "$sbsign" --key "$private_key" --cert "$public_key" --output "$dst" "$src" >/dev/null
    }

    install_signed_or_unsigned "$decider_dst"

    if [ ! -e "$decider_dst" ]; then
      echo "decider: failed to install $decider_dst" >&2
      exit 1
    fi

    "$mkdir" -p "$esp_mount/decider"
    "$install" -m 0644 "$conf_src" "$esp_mount/decider/decider.conf"

    if [ ${lib.boolToString cfg.efiInstallAsRemovable} = true ]; then
      install_signed_or_unsigned "$fallback_dst"
    fi

    ${lib.optionalString efi.canTouchEfiVariables ''
      if [ "$esp_mount" = ${lib.escapeShellArg efi.efiSysMountPoint} ]; then
        ${updateEfiBootEntry}
      fi
    ''}
  '';

  mkLanzabooteInstallCommand = efiSysMountPoint: ''
    ${lanzaboote.installCommand} \
      --public-key ${lanzaboote.publicKeyFile} \
      --private-key ${lanzaboote.privateKeyFile} \
      ${efiSysMountPoint} \
      /nix/var/nix/profiles/system-*-link
  '';
in
{
  options.boot.loader.decider = {
    enable = lib.mkEnableOption "install decider.efi as the primary systemd-boot chainloader";

    package = lib.mkOption {
      type = lib.types.package;
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

    choiceSource = lib.mkOption {
      type = lib.types.enum [
        "fs"
        "tftp"
      ];
      default = "fs";
      description = ''
        Source used to retrieve DECIDER.CHO choice data.
      '';
    };

    tftpIp = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "10.10.0.2";
      description = ''
        TFTP server IP (IPv4 or IPv6) used when `choiceSource = "tftp"`.
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
            assertion = systemdBootEnabled || lanzabooteEnabled;
            message = "boot.loader.decider.enable requires either boot.loader.systemd-boot.enable or boot.lanzaboote.enable.";
          }
          {
            assertion = (cfg.choiceSource != "tftp") || (cfg.tftpIp != null);
            message = "boot.loader.decider.tftpIp must be set when boot.loader.decider.choiceSource = \"tftp\".";
          }
        ];
      }

      (lib.mkIf systemdBootEnabled {
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

      (lib.mkIf (systemdBootEnabled && efi.canTouchEfiVariables) {
        boot.loader.systemd-boot.extraInstallCommands = lib.mkAfter ''
          ${updateEfiBootEntry}
        '';
      })

      (
        if hasLanzabooteOption then
          lib.mkIf lanzabooteEnabled {
            boot.loader.external.installHook = lib.mkForce (
              pkgs.writeShellScript "bootinstall" (
                lib.concatStringsSep "\n" (
                  map (efiSysMountPoint: ''
                    ${mkLanzabooteInstallCommand efiSysMountPoint}
                    ${installDeciderLanzaboote} ${lib.escapeShellArg efiSysMountPoint}
                  '') lanzabooteEfiSysMountPoints
                )
              )
            );

            systemd.services.prepare-sb-auto-enroll.script = lib.mkIf lanzaboote.autoEnrollKeys.enable (
              lib.mkAfter ''
                ${installDeciderLanzaboote} ${lib.escapeShellArg efi.efiSysMountPoint}
              ''
            );
          }
        else
          { }
      )
    ]
  );
}
