{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
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
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        hostArch = pkgs.stdenv.hostPlatform.qemuArch;
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

        mkDecider =
          targetArch:
          craneLib.buildPackage {
            pname = "decider";
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
          decider-x86_64 = mkDecider "x86_64";
          decider-aarch64 = mkDecider "aarch64";
          decider = mkDecider hostArch;
          default = mkDecider hostArch;
        };

        apps =
          let
            qemuSettings = {
              mode = "entry";
              entry = "auto-reboot-to-firmware-setup";
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
              "decider"
            ];
            appChecks = pkgs.lib.mapAttrs (_: app: app.qemuScript) (
              removeAttrs apps [
                "default"
                "qemu"
              ]
            );
          in
          packageChecks // appChecks;

        packages = packages;
        apps = apps;
      }
    );
}
