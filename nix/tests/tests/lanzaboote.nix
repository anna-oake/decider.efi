{
  name = "lanzaboote";

  node.pkgsReadOnly = false;

  nodes.machine =
    {
      lib,
      lanzaboote,
      ...
    }:
    {
      imports = [ lanzaboote.nixosModules.lanzaboote ];

      virtualisation.useSecureBoot = true;

      boot.loader.systemd-boot.enable = lib.mkForce false;
      boot.lanzaboote = {
        enable = true;
        pkiBundle = "/var/lib/lanzaboote-auto-generated";
        autoGenerateKeys.enable = true;
        autoEnrollKeys.enable = true;
        allowUnsigned = true;
      };
    };

  testScript = ''
    machine.start(allow_reboot=True)

    machine.wait_for_console_text("resolved nixos-current to entry id: nixos-generation-1", 30)
    machine.wait_for_console_text("Loaded initrd from LINUX_EFI_INITRD_MEDIA_GUID device path", 10)
    # alright so decider works

    machine.wait_for_unit("prepare-sb-auto-enroll.service")
    # now we manually reboot the vm to enroll keys
    machine.reboot()

    machine.wait_for_unit("multi-user.target")
  '';
}
