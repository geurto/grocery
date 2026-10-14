{
  description = "grocery: shopping lists and nutrition from Cooklang recipes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux" # Raspberry Pi
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            # Rust (workspace MSRV is 1.80; nixpkgs ships a newer stable)
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer

            # Web app (web/). Svelte, Vite and TypeScript are npm
            # devDependencies pinned in web/package-lock.json, not Nix
            # packages, so `npm ci` gives everyone the same versions.
            nodejs_22

            # `sqlite3 grocery.sqlite ".backup ..."` and poking at the database.
            sqlite
          ];

          # rust-analyzer needs the standard library sources.
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
