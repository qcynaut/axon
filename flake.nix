{
  description = "Axon protocol";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            cargo-audit
            cargo-deny
            just
            ripgrep
            fd
            jq
            git
          ];

          buildInputs = with pkgs; [
            # add system libs here: openssl, protobuf, etc.
          ];

          RUST_BACKTRACE = "1";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

          shellHook = ''
            if [ -z "$NIX_FISH_SHELL" ]; then
              export NIX_FISH_SHELL=1
              exec ${pkgs.fish}/bin/fish
            fi
          '';
        };
      }
    );
}
