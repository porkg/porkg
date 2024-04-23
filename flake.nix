{
  description = "Porkg Package Manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nci = {
      url = "github:yusdacra/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      imports = [
        inputs.nci.flakeModule
        ./crates.nix
      ];
      systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin"];
      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: let
        outputs = config.nci.outputs;
      in {
        packages.default = outputs.nip.packages.release;
        devShells.default = outputs.porkg.devShell.overrideAttrs (old: {
          packages = with pkgs; (old.packages or []) ++ [cargo-expand gdb cargo-udeps curl jq zstd just];
          RUST_LOG = "trace";
          shellHook = ''
            declare -a parts
            try_find() {
              id=$1
              fn=$2
              parts=( )
              if line=$(cat "$fn" | grep -E "^$id:[0-9]+:[0-9]+\$" | grep -oE '[0-9]+:[0-9]+$'); then
                IFS=':' read -r -a parts <<< "$line"
                start="''${parts[0]}"
                length="''${parts[1]}"
                end=$(($start + $length))
                echo "$start $end"
                return 0
              fi
              return 1
            }

            if user=$(try_find $(id -u) /etc/subuid) || user=$(try_find $(id -un) /etc/subuid); then
              parts=( )
              IFS=' ' read -r -a parts <<< "$user"
              export PORK__DAEMON__SUB_UID__MIN=''${parts[0]}
              export PORK__DAEMON__SUB_UID__MAX=''${parts[1]}
            fi

            if group=$(try_find $(id -g) /etc/subgid) || group=$(try_find $(id -gn) /etc/subgid); then
              parts=( )
              IFS=' ' read -r -a parts <<< "$group"
              export PORK__DAEMON__SUB_GID__MIN=''${parts[0]}
              export PORK__DAEMON__SUB_GID__MAX=''${parts[1]}
            fi
          '';
        });
        formatter = pkgs.alejandra;
      };
      flake = {
        # The usual flake attributes can be defined here, including system-
        # agnostic ones like nixosModule and system-enumerating ones, although
        # those are more easily expressed in perSystem.
      };
    };
}
