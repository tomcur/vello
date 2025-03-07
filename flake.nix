{
  description = "A modular signal generator";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };
  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        lib = pkgs.lib;
      in
      {
        # For an example of packaging Skia with Nixpkgs,
        # see: https://github.com/NixOS/nixpkgs/blob/8f0c1b40c5f3350ae62e29d2f06a9c5d4994f89e/pkgs/applications/editors/neovim/neovide/default.nix
        devShell = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } rec {
          SKIA_GN_COMMAND = "${pkgs.gn}/bin/gn";
          SKIA_NINJA_COMMAND = "${pkgs.ninja}/bin/ninja";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          VK_LAYER_PATH = "${pkgs.vulkan-validation-layers}/share/vulkan/explicit_layer.d";
          nativeBuildInputs = with pkgs; [
            clang
            cmake
            # rust-analyzer
            (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            # cargo
            # rustc
            # rustup
            rustfmt
            pkg-config
            python3 # skia
            # Vulkan
            # cmake
            shaderc
            vulkan-headers
            vulkan-loader
            vulkan-tools
            vulkan-validation-layers
            # For Criterion benchmarks
            gnuplot
            # Changelog generator
            git-cliff
          ];
          buildInputs = with pkgs; [
            libjack2
            libGL
            libxkbcommon
            fontconfig
            freetype
            expat
            # swiftshader
            bzip2
            libpng
            brotli
          ] ++ (with pkgs.xorg; [
            libX11
            libXcursor
            libXrandr
            libXi
            xcbutilwm
          ]);
          shellHook =
            let
              libraryPath = with pkgs;
                lib.strings.makeLibraryPath [
                  vulkan-loader
                  vulkan-validation-layers
                  libGL
                  pkgs.bzip2
                  pkgs.libpng
                  pkgs.brotli
                  # why doesn't winit find xkbcommon by itself anymore since v0.29? I assume it did find it before?
                  pkgs.libxkbcommon

                  wayland

                  # See:
                  # https://github.com/iced-rs/iced/blob/master/DEPENDENCIES.md
                  expat
                  fontconfig
                  freetype
                  freetype.dev
                  libGL
                  pkg-config
                  xorg.libX11
                  xorg.libXcursor
                  xorg.libXi
                  xorg.libXrandr
                ];
            in
            ''
              RUST_SRC_PATH="${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
              export RUST_LOG="warn,response=trace,response_app=debug,response_view=debug,skimgui=trace";
              # workaround for npm dep compilation
              # https://github.com/imagemin/optipng-bin/issues/108

              LD_LIBRARY_PATH=$LD_LIBRARY_PATH:${libraryPath}
              LD=$CC
            '';
        };
      });
}
