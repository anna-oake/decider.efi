{
  name = "systemd-boot";

  node.pkgsReadOnly = false;

  nodes.machine = {
    boot.loader.systemd-boot.enable = true;
  };

  testScript = ''
    machine.start()
    machine.wait_for_console_text("resolved nixos-current to entry id: nixos-generation-1", 30)
    machine.wait_for_console_text("Loaded initrd from LINUX_EFI_INITRD_MEDIA_GUID device path", 10)
  '';
}
