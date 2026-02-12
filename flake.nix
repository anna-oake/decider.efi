{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    lanzaboote = {
      url = "github:nix-community/lanzaboote/1902463415745b992dbaf301b2a35a1277be1584";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      crane,
      lanzaboote,
      rust-overlay,
    }:
    let
      rustOverlay = import rust-overlay;

      perSystem = flake-utils.lib.eachDefaultSystem (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rustOverlay ];
          };

          hostArch = pkgs.stdenv.hostPlatform.qemuArch;
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

          mkDecider =
            targetArch:
            craneLib.buildPackage {
              pname = "decider-efi";
              version = "0.0.1";

              src = craneLib.cleanCargoSource ./.;

              doCheck = false;
              CARGO_BUILD_TARGET = "${targetArch}-unknown-uefi";

              # Workaround for crane dependency builds on no_std/no_main UEFI crates.
              dummyrs = pkgs.writeText "dummy.rs" ''
                #![allow(unused)]
                #![cfg_attr(any(target_os = "none", target_os = "uefi"), no_std, no_main)]

                #[cfg_attr(any(target_os = "none", target_os = "uefi"), panic_handler)]
                fn panic(_info: &::core::panic::PanicInfo<'_>) -> ! {
                  loop {}
                }

                #[cfg_attr(any(target_os = "none", target_os = "uefi"), unsafe(export_name = "efi_main"))]
                fn main() {}
              '';
            };

          qemu = import ./qemu.nix {
            inherit
              pkgs
              nixpkgs
              mkDecider
              ;
          };

          packages = {
            decider-efi-x86_64 = mkDecider "x86_64";
            decider-efi-aarch64 = mkDecider "aarch64";
            decider-efi = mkDecider hostArch;
            default = mkDecider hostArch;
          };

          apps =
            let
              qemuSettings = {
                choiceType = "entry_id";
                entryId = "auto-reboot-to-firmware-setup";
              };
            in
            {
              qemu-x86_64 = qemu.mkQemuApp "x86_64" qemuSettings;
              qemu-aarch64 = qemu.mkQemuApp "aarch64" qemuSettings;
              qemu = qemu.mkQemuApp hostArch qemuSettings;
              default = qemu.mkQemuApp hostArch qemuSettings;
            };
        in
        {
          checks =
            let
              packageChecks = removeAttrs packages [
                "default"
                "decider-efi"
              ];
              appChecks = pkgs.lib.mapAttrs (_: app: app.qemuScript) (
                removeAttrs apps [
                  "default"
                  "qemu"
                ]
              );
              testChecks = pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (
                pkgs.lib.mapAttrs' (name: value: pkgs.lib.nameValuePair "test-${name}" value) (
                  import ./nix/tests {
                    inherit pkgs;
                    deciderModule = self.nixosModules.decider;
                    inherit lanzaboote;
                  }
                )
              );
            in
            packageChecks // appChecks // testChecks;

          inherit packages apps;
        }
      );
    in
    perSystem
    // {
      nixosModules = {
        decider =
          args@{ pkgs, ... }:
          import ./nix/modules/decider.nix (
            args
            // {
              deciderPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.decider-efi;
            }
          );
        default = self.nixosModules.decider;
      };
    };
}
