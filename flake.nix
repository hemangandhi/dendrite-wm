{
  description = "A flake for building the Dendrite Smithay-based Wayland compositor";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        runtimeLibs = with pkgs; [
          wayland
          libxkbcommon
          libinput
          libudev-zero       # or systemd (libudev)
          seatd
          pixman
          libGL
          libgbm             # TODO: does this replace one of the others?
          vulkan-loader
          xorg.libX11        # Required if nesting inside X11/Winit backend
          xorg.libXcursor
          xorg.libXi
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          # nativeBuildInputs are tools that execute on the host machine during compilation
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            pkg-config       # Absolutely critical for Rust build scripts to find C headers
            wayland-scanner  # Generates glue-code for extra protocols
          ];

          buildInputs = runtimeLibs;

          shellHook = ''
            # Make sure rustc can link dynamically to graphics layers during execution
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
            echo "❄️  Nix Development Environment for Dendrite Loaded Successfully! ❄️"
          '';
        };
      });
}
