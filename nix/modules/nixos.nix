{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.rudo;
in {
  options.programs.rudo = {
    enable = lib.mkEnableOption "rudo terminal emulator";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.default;
      defaultText = lib.literalExpression "inputs.rudo.packages.${pkgs.system}.default";
      description = "The rudo package to add to environment.systemPackages.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
