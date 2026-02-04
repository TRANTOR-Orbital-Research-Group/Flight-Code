{
  description = "Trantor Flight Code - Pico 2 Dev Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = [ "rust-src" "rust-analyzer" ];
        targets = [ "thumbv8m.main-none-eabihf" ];
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          rustToolchain
          
          probe-rs-tools 
          elf2uf2-rs
          
          pkg-config
          udev
        ];

        shellHook = ''
          echo "Target: thumbv8m.main-none-eabihf (Pico 2 / RP2350)"
          echo "Rust: $(rustc --version)"
        '';
      };
    };
}



#  Distro:  Nix  hell:  Ⱥ Fish      sa9m@higgs-boson TRANTOR/Flight-Code on branchD $!?⇡ via 呂 v1.92.0 ➜       