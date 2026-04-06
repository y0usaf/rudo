{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.rudo;
  tomlFormat = pkgs.formats.toml { };
in {
  options.programs.rudo = {
    enable = lib.mkEnableOption "rudo terminal emulator";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.default;
      defaultText = lib.literalExpression "inputs.rudo.packages.${pkgs.system}.default";
      description = "The rudo package to install.";
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      example = {
        font = {
          family = "JetBrains Mono";
          size = 14.0;
        };
        cursor = {
          style = "block";
          blink = true;
        };
        window.padding = 2;
      };
      description = "Configuration written to ~/.config/rudo/config.toml.";
    };

    theme = lib.mkOption {
      type = lib.types.nullOr tomlFormat.type;
      default = null;
      example = {
        foreground = "#c0caf5";
        background = "#1a1b26";
        cursor = "#c0caf5";
        selection = "#33467c";
      };
      description = "Optional theme written to ~/.config/rudo/theme.toml.";
    };
  };

  config = lib.mkIf cfg.enable (({
    home.packages = [ cfg.package ];
    xdg.configFile."rudo/config.toml".source = tomlFormat.generate "rudo-config.toml" cfg.settings;
  }) // lib.optionalAttrs (cfg.theme != null) {
    xdg.configFile."rudo/theme.toml".source = tomlFormat.generate "rudo-theme.toml" cfg.theme;
  });
}
