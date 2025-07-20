{
  nixConfig = {
    extra-substituters = [ "https://om.cachix.org" ];
    extra-trusted-public-keys = [ "om.cachix.org-1:ifal/RLZJKN4sbpScyPGqJ2+appCslzu7ZZF/C01f2Q=" ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-utils.url = "github:numtide/flake-utils";

    pre-commit-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane = {
      url = "github:ipetkov/crane";
    };

    omnix = {
      url = "github:juspay/omnix";
      # We do not follow nixpkgs here, because then we can't use the omnix cache
      # inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      flake-utils,
      pre-commit-hooks,
      crane,
      omnix,
      nixpkgs,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
          overlays = [
            (final: prev: {
              inherit (omnix.packages.${final.system}) omnix-cli;
            })
          ];
        };
        lib = pkgs.lib;
        craneLib = crane.mkLib pkgs;
        preCommitHooksLib = pre-commit-hooks.lib.${system};

        # Common arguments can be set here to avoid repeating them later
        # Note: changes here will rebuild all dependency crates
        src = craneLib.cleanCargoSource ./.;
        commonArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
          ];

          buildInputs =
            with pkgs;
            [
              # Add additional build inputs here
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
            ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        cargo-featurex = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        pre-commit-check = preCommitHooksLib.run {
          src = ./.;
          hooks = {
            flake-checker.enable = true;

            clippy = {
              enable = true;
              settings.denyWarnings = true;
              settings.extraArgs = "--all";
              settings.offline = false;
            };

            nixfmt-rfc-style.enable = true;
          };
        };

      in
      rec {
        checks = {
          inherit cargo-featurex;

          # Run clippy (and deny all warnings) on the workspace source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          workspace-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          # Check formatting
          workspace-fmt = craneLib.cargoFmt {
            inherit src;
          };

          devShell-with-featurex = craneLib.devShell {
            packages = [ packages.cargo-featurex ];
          };
        };

        packages = {
          inherit cargo-featurex;

          default = packages.cargo-featurex;
        };

        apps = {
          cargo-featurex = flake-utils.lib.mkApp {
            drv = cargo-featurex;
          };

          default = apps.cargo-featurex;
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          inherit checks;
          inherit (pre-commit-check) shellHook;

          buildInputs = pre-commit-check.enabledPackages;

          packages = with pkgs; [
            cargo-autoinherit
            cargo-expand
            cargo-workspaces
            cargo-nextest
            just
            jq
            omnix-cli
          ];
        };
      }
    )

    # CI configuration
    // {
      om.ci = {
        default = {
          root = {
            dir = ".";
            steps = {
              # The build step is enabled by default. It builds all flake outputs.
              build.enable = true;
            };
          };
        };
      };
    };
}
