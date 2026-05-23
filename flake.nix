{
  description = "Axon protocol";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            fish
            direnv
            nix-direnv
            git
            ripgrep
            fd
            jq
            just
          ];

          shellHook = ''
            if [ -z "$NIX_FISH_SHELL" ]; then
                export NIX_FISH_SHELL=1
                exec ${pkgs.fish}/bin/fish
            fi
            echo "Dev Shell Loaded"
          '';
        };
      }
    );
}
