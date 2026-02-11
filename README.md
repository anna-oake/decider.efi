vibecoded efi shim

catcoded and less cursed solution of the same problem for grub: [eule-booter](https://github.com/anna-oake/eule-booter)

1. install systemd-boot
2. install decider.efi
3. set decider.efi as default boot option
4. you can now set `LoaderEntryOneShot` on boot using a removable flash drive with DECIDER.CHO in its root

hint: the removable flash drive should be an esp32/rp2040 pretending to be a usb drive and serving the right file depending on some physical toggle state or whatever

todo:
- (maybe?) throw out qemu.nix stuff
- refactor the flake to something less ugly
- support other bootloaders (big maybe??? other bootloaders are smart enough to allow this out of the box, see [eule-booter](https://github.com/anna-oake/eule-booter))
- learn some rust and see if this code is shit, do some refactoring (never?)

use case:
- remotely choose an OS to boot your PC into BEFORE it boots
