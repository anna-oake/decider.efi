{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
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
      crane,
      rust-overlay,
    }:
    let
      rustOverlay = import rust-overlay;

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      mkPerSystem =
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

          packages = {
            decider-efi-x86_64 = mkDecider "x86_64";
            decider-efi-aarch64 = mkDecider "aarch64";
            decider-efi = mkDecider hostArch;
            default = mkDecider hostArch;
          };
        in
        {
          checks = removeAttrs packages [
            "default"
            "decider-efi"
          ];

          inherit packages;
        };

      perSystem = nixpkgs.lib.genAttrs systems mkPerSystem;
    in
    {
      packages = nixpkgs.lib.mapAttrs (_: value: value.packages) perSystem;
      checks = nixpkgs.lib.mapAttrs (_: value: value.checks) perSystem;

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
