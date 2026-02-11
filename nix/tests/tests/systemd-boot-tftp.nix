{ pkgs, ... }:
let
  tftpRoot = pkgs.runCommand "decider-tftp-root" { } ''
    mkdir -p "$out"
    cat > "$out/DECIDER.CHO" <<'EOF'
    choice_type=nixos-current
    EOF
  '';
in
{
  name = "systemd-boot-tftp";

  node.pkgsReadOnly = false;

  nodes.machine =
    { lib, ... }:
    {
      boot.loader.systemd-boot.enable = true;
      boot.loader.decider = {
        choiceSource = "tftp";
        tftpIp = "10.0.2.2";
      };
      virtualisation.vlans = lib.mkForce [ ];

      virtualisation.qemu.networkingOptions = lib.mkForce [
        "-net nic,netdev=user.0,model=e1000"
        # bootfile here doesn't matter at all, but pxe in qemu doesn't work otherwise
        "-netdev user,id=user.0,tftp=${tftpRoot},bootfile=whatever"
      ];
    };

  testScript = ''
    machine.start()
    machine.wait_for_console_text("resolved nixos-current to entry id: nixos-generation-1", 120)
    machine.wait_for_console_text("Loaded initrd from LINUX_EFI_INITRD_MEDIA_GUID device path", 10)
  '';
}
