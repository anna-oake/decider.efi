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
  systemdBootEnabled = config.boot.loader.systemd-boot.enable;

  lzbtCfg =
    if options.boot ? lanzaboote then
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

  efiArch = pkgs.stdenv.hostPlatform.efiArch;
  deciderEfiName = "decider${efiArch}.efi";
  fallbackEfiName = "BOOT${lib.toUpper efiArch}.EFI";
  chainloadPathLines = lib.mapAttrsToList (
    name: path: "chainload_${name}_path=${path}"
  ) cfg.chainloadPaths;

  deciderConfig = pkgs.writeText "decider.conf" ''
    chainload_systemd_path=${cfg.chainloadSystemdPath}
    choice_source=${cfg.choiceSource}
    ${lib.optionalString (cfg.choiceSource == "tftp") "tftp_ip=${cfg.tftpIp}"}
    ${lib.concatStringsSep "\n" chainloadPathLines}
  '';

  updateEfiBootEntry = pkgs.writeShellApplication {
    name = "decider-update-efi-boot-entry";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.efibootmgr
      pkgs.util-linux
    ];
    text = ''
      label='decider.efi'
      loader_path='/EFI/decider/${deciderEfiName}'

      part_device="$(readlink -f "$(findmnt -nro SOURCE --target "${efi.efiSysMountPoint}")")"
      part_name="''${part_device##*/}"
      part_sysfs="/sys/class/block/$part_name"

      part_number="$(cat "$part_sysfs/partition" 2>/dev/null || true)"
      disk_name="$(basename "$(readlink -f "$part_sysfs/..")" 2>/dev/null || true)"

      if [ -z "$disk_name" ] || [ -z "$part_number" ]; then
        echo "decider: failed to resolve ESP disk/partition for ${efi.efiSysMountPoint} ($part_device)" >&2
        exit 1
      fi

      disk_device="/dev/$disk_name"

      efibootmgr -B -L "$label" >/dev/null 2>&1 || true

      efibootmgr \
        -c \
        -d "$disk_device" \
        -p "$part_number" \
        -l "$loader_path" \
        -L "$label" >/dev/null
    '';
  };

  installDeciderLanzaboote = pkgs.writeShellApplication {
    name = "decider-install-lanzaboote";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.sbsigntool
    ];
    text = ''
      esp_mount='${efi.efiSysMountPoint}'
      src='${cfg.package}/bin/decider.efi'
      conf_src='${deciderConfig}'
      decider_dst='${efi.efiSysMountPoint}/EFI/decider/${deciderEfiName}'
      decider_conf_dir='${efi.efiSysMountPoint}/decider'

      if [ ! -r "$src" ]; then
        echo "decider: source EFI binary not found: $src" >&2
        exit 1
      fi

      echo "decider: installing via lanzaboote to ESP at $esp_mount" >&2

      install_image() {
        local dst="$1"
        mkdir -p "$(dirname "$dst")"

        if sbsign \
          --key '${lzbtCfg.privateKeyFile}' \
          --cert '${lzbtCfg.publicKeyFile}' \
          --output "$dst" \
          "$src" >/dev/null 2>&1; then
          return 0
        fi

        ${
          if lzbtCfg.allowUnsigned then
            ''
              install -m 0644 "$src" "$dst"
            ''
          else
            ''
              echo "decider: failed to sign decider.efi (set boot.lanzaboote.allowUnsigned = true to allow unsigned install)" >&2
              return 1
            ''
        }
      }

      install_image "$decider_dst"

      mkdir -p "$decider_conf_dir"
      install -m 0644 "$conf_src" "$decider_conf_dir/decider.conf"

      ${lib.optionalString cfg.efiInstallAsRemovable ''
        install_image "${efi.efiSysMountPoint}/EFI/BOOT/${fallbackEfiName}"
      ''}

      ${lib.optionalString efi.canTouchEfiVariables (lib.getExe updateEfiBootEntry)}
    '';
  };

  mkLanzabooteInstallCommand = efiSysMountPoint: ''
    ${lzbtCfg.installCommand} \
      --public-key ${lzbtCfg.publicKeyFile} \
      --private-key ${lzbtCfg.privateKeyFile} \
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

    chainloadSystemdPath = lib.mkOption {
      type = lib.types.str;
      default = "/EFI/systemd/systemd-boot${efiArch}.efi";
      defaultText = lib.literalExpression ''"\\EFI\\systemd\\systemd-boot''${pkgs.stdenv.hostPlatform.efiArch}.efi"'';
      description = ''
        UEFI path that decider will chainload after setting LoaderEntryOneShot.
        Supports plain paths like `\EFI\systemd\systemd-bootx64.efi` (from the
        current boot device) and prefixed paths like
        `ce6a9709-944e-4496-9363-1706dac399ee:/EFI/...` to select a GPT partition
        by GUID.
      '';
    };

    chainloadPaths = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = lib.literalExpression ''
        {
          windows = "ce6a9709-944e-4496-9363-1706dac399ee:/EFI/Microsoft/Boot/bootmgfw.efi";
        }
      '';
      description = ''
        Additional chainload path mappings rendered to {file}`decider.conf` as
        `chainload_<name>_path=<value>`.

        For example, setting
        `chainloadPaths.windows = "ce6a9709-944e-4496-9363-1706dac399ee:/EFI/..."`
        emits
        `chainload_windows_path=ce6a9709-944e-4496-9363-1706dac399ee:/EFI/...`,
        which can be selected via
        `choice_type=chainload_windows` in {file}`DECIDER.CHO`.
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
            assertion = systemdBootEnabled || lzbtCfg.enable;
            message = "boot.loader.decider.enable requires either boot.loader.systemd-boot.enable or boot.lanzaboote.enable.";
          }
          {
            assertion = (cfg.choiceSource != "tftp") || (cfg.tftpIp != null);
            message = "boot.loader.decider.tftpIp must be set when boot.loader.decider.choiceSource = \"tftp\".";
          }
          {
            assertion = (!lzbtCfg.enable) || (lzbtCfg.extraEfiSysMountPoints == [ ]);
            message = "boot.loader.decider with lanzaboote currently supports only the primary EFI system mount; boot.lanzaboote.extraEfiSysMountPoints must be empty.";
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
          ${lib.getExe updateEfiBootEntry}
        '';
      })

      (lib.mkIf lzbtCfg.enable {
        boot.loader.external.installHook = lib.mkForce (
          pkgs.writeShellScript "bootinstall" ''
            ${mkLanzabooteInstallCommand efi.efiSysMountPoint}
            ${lib.getExe installDeciderLanzaboote}
          ''
        );

        systemd.services.prepare-sb-auto-enroll.script = lib.mkIf lzbtCfg.autoEnrollKeys.enable (
          lib.mkAfter ''
            ${lib.getExe installDeciderLanzaboote}
          ''
        );
      })
    ]
  );
}
