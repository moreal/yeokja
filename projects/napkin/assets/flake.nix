{
  description = "An Infinitely Large Napkin — Korean translation";
  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    with flake-utils.lib;
      eachSystem [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ] (
        system: let
          pkgs = import nixpkgs {inherit system;};
          notoSansCjk = pkgs.noto-fonts-cjk-sans.override {static = true;};
          notoSerifCjk = pkgs.noto-fonts-cjk-serif.override {static = true;};
          tex = pkgs.texlive.combined.scheme-full;
        in {
          formatter = pkgs.alejandra;
          defaultPackage = pkgs.stdenv.mkDerivation {
            name = "napkin-ko";
            version = "1.6";
            src = ./.;
            buildInputs = with pkgs; [
              asymptote
              biber
              ghostscript
              notoSansCjk
              notoSerifCjk
              tex
            ];
            buildPhase = ''
              export OSFONTDIR="${notoSansCjk}/share/fonts//:${notoSerifCjk}/share/fonts//"
              export XDG_CACHE_HOME="cache"
              export TEXMFCACHE="texmf-cache"
              export TEXMFVAR="texmf-var"
              mkdir -p "$XDG_CACHE_HOME" "$TEXMFCACHE" "$TEXMFVAR"
              mkdir -p asy
              latexmk -lualatex -f -interaction=nonstopmode
            '';
            installPhase = ''
              mkdir -p $out
              cp Napkin.pdf $out/
            '';
          };
          devShell = pkgs.mkShell {
            packages = with pkgs; [
              asymptote
              biber
              ghostscript
              mathjax
              notoSansCjk
              notoSerifCjk
              perlPackages.LaTeXML
              python3
              tex
            ];
            FONTCONFIG_FILE = pkgs.makeFontsConf {
              fontDirectories = [notoSansCjk notoSerifCjk];
            };
            shellHook = ''
              export OSFONTDIR="${notoSansCjk}/share/fonts//:${notoSerifCjk}/share/fonts//"
              export NAPKIN_MATHJAX_DIR="${pkgs.mathjax}/lib/node_modules/mathjax"
            '';
          };
        }
      );
}
