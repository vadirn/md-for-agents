{pkgs ? import <nixpkgs> {}}:
pkgs.mkShell {
  packages = with pkgs; [
    cargo
    rustc
    rustfmt
    clippy
    rust-analyzer
    # Nix's rustc calls `lld` to link wasm32; rustup toolchains bundle it instead.
    lld
  ];
}
