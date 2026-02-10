{
  pkgs,
  nixpkgs,
  mkDecider,
}:

rec {
  mkFatImage =
    imageName: srcDrv:
    pkgs.runCommand "decider-fat-image-${imageName}"
      {
        nativeBuildInputs = [
          pkgs.dosfstools
          pkgs.mtools
        ];
      }
      ''
        set -euo pipefail

        img="$TMPDIR/${imageName}"
        truncate -s 64M "$img"
        mkfs.vfat -F 32 "$img" >/dev/null
        mcopy -s -i "$img" "${srcDrv}"/* ::

        mkdir -p "$out"
        cp "$img" "$out/${imageName}"
      '';

  mkEspImage =
    targetArch:
    let
      targetLinuxPkgs = import nixpkgs {
        system = "${targetArch}-linux";
      };
      deciderPkg = mkDecider targetArch;
      systemdBootEfiName =
        if targetArch == "aarch64" then "systemd-bootaa64.efi" else "systemd-bootx64.efi";
      uefiBootFile = if targetArch == "aarch64" then "BOOTAA64.EFI" else "BOOTX64.EFI";
      deciderConf = pkgs.writeText "decider.conf" ''
        chainload_path=\EFI\systemd\${systemdBootEfiName}
        entries_path=\loader\entries
      '';
      loaderConf = pkgs.writeText "loader.conf" ''
        timeout 1
        console-mode keep
      '';
      emptyEntry = pkgs.writeText "empty-entry.conf" "";
      espRoot = pkgs.linkFarm "decider-esp-root-${targetArch}" [
        {
          name = "EFI/BOOT/${uefiBootFile}";
          path = "${deciderPkg}/bin/decider.efi";
        }
        {
          name = "EFI/decider/decider.efi";
          path = "${deciderPkg}/bin/decider.efi";
        }
        {
          name = "EFI/systemd/${systemdBootEfiName}";
          path = "${targetLinuxPkgs.systemd}/lib/systemd/boot/efi/${systemdBootEfiName}";
        }
        {
          name = "decider/decider.conf";
          path = deciderConf;
        }
        {
          name = "loader/loader.conf";
          path = loaderConf;
        }
        {
          name = "loader/entries/random-stuff.conf";
          path = emptyEntry;
        }
        {
          name = "loader/entries/nixos-generation-25.conf";
          path = emptyEntry;
        }
        {
          name = "loader/entries/nixos-generation-34.conf";
          path = emptyEntry;
        }
      ];
    in
    mkFatImage "esp.img" espRoot;

  mkUsbImage =
    targetArch:
    {
      mode,
      entry,
    }:
    mkFatImage "usb.img" (
      pkgs.writeTextFile {
        name = "decider-usb-root-${targetArch}";
        destination = "/decider.choice";
        text = ''
          mode=${mode}
          entry=${entry}
        '';
      }
    );

  mkQemuApp =
    targetArch:
    {
      mode ? "entry",
      entry ? "auto-reboot-to-firmware-setup",
    }:
    let
      machineType = if targetArch == "aarch64" then "virt" else "pc";

      ovmfCode = "${pkgs.qemu}/share/qemu/edk2-${targetArch}-code.fd";

      espImage = mkEspImage targetArch;
      usbImage = mkUsbImage targetArch { inherit mode entry; };

      qemuCommand = ''
        "${pkgs.qemu}/bin/qemu-system-${targetArch}" \
          -machine ${machineType} \
          -cpu max \
          -m 128 \
          -display none \
          -monitor none \
          -global driver=virtio-net-pci,property=romfile,value="" \
          -drive if=pflash,format=raw,file="${ovmfCode}",readonly=on \
          -drive format=raw,file="$ESP_IMG" \
          -drive format=raw,file="$USB_IMG" \
          -serial stdio
      '';

      qemuScript = pkgs.writeShellApplication {
        name = "decider-qemu-${targetArch}";
        text = ''
          RUN_DIR="$(mktemp -d)"
          ESP_IMG="$RUN_DIR/esp.img"
          USB_IMG="$RUN_DIR/usb.img"

          cleanup() {
            echo "wiping $RUN_DIR"
            rm -rf "$RUN_DIR"
          }
          trap cleanup EXIT

          echo "preparing image files for qemu ${targetArch} in $RUN_DIR..."
          cp "${espImage}/esp.img" "$ESP_IMG"
          cp "${usbImage}/usb.img" "$USB_IMG"
          chmod u+w "$ESP_IMG" "$USB_IMG"

          echo "booting qemu ${targetArch}..."
          ${qemuCommand}
        '';
      };
    in
    {
      type = "app";
      program = "${qemuScript}/bin/decider-qemu-${targetArch}";
      meta.description = "Run decider QEMU harness for ${targetArch} guest";
    };
}
