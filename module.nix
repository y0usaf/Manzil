# manzil — minimalist replacement for Home Manager's `home.files`.
{ config, lib, pkgs, ... }:
let
  inherit (lib)
    attrNames attrValues concatMapStringsSep filter filterAttrs flatten
    literalExpression mapAttrsToList mkDefault mkEnableOption mkIf mkOption
    pipe replaceStrings stringAfter;
  inherit (lib.types) attrsOf bool lines nullOr path str submodule;

  osConfig = config; # outer NixOS config — shadowed inside submodules
  cfg = config.manzil;

  # One file entry under `manzil.users.<name>.files."<path>"`.
  fileType = userCfg: submodule ({ name, config, ... }: {
    options = {
      enable = mkEnableOption "this file" // { default = true; };

      target = mkOption {
        type = str;
        default = name;
        description = "Path relative to the user's home directory.";
      };

      text = mkOption {
        type = nullOr lines;
        default = null;
        description = "Inline file contents.";
      };

      source = mkOption {
        type = nullOr path;
        default = null;
        description = "Source file or directory to symlink.";
      };

      executable = mkOption {
        type = bool;
        default = false;
        description = "Set the +x bit (only meaningful with `text`).";
      };

      clobber = mkOption {
        type = bool;
        default = userCfg.clobberByDefault;
        description = "Overwrite an existing non-symlink target.";
      };
    };

    config = mkIf (config.text != null) {
      source = mkDefault (pkgs.writeTextFile {
        name = "manzil-${replaceStrings [ "/" ] [ "-" ] name}";
        inherit (config) text executable;
      });
    };
  });

  userType = submodule ({ name, config, ... }: {
    options = {
      enable = mkEnableOption "manzil for this user" // { default = true; };

      directory = mkOption {
        type = path;
        default = osConfig.users.users.${name}.home or "/home/${name}";
        defaultText = literalExpression "users.users.<name>.home";
        description = "Home directory file targets are relative to.";
      };

      clobberByDefault = mkOption {
        type = bool;
        default = cfg.clobberByDefault;
        description = "Default value for `files.<path>.clobber`.";
      };

      files = mkOption {
        type = attrsOf (fileType config);
        default = { };
        example = literalExpression ''
          {
            ".bashrc".text = "alias ll='ls -la'";
            ".config/foo".source = ./foo;
            ".local/bin/x" = { source = ./x.sh; executable = true; };
          }
        '';
        description = "Files to manage in the user's home directory.";
      };
    };
  });

  mkManifest = name: userCfg:
    let
      entries = pipe userCfg.files [
        attrValues
        (filter (f: f.enable && f.source != null))
        (map (f: {
          target  = "${userCfg.directory}/${f.target}";
          source  = "${f.source}";
          clobber = f.clobber;
        }))
      ];
    in pkgs.writeText "manzil-manifest-${name}.json"
      (builtins.toJSON { files = entries; });

  linker = pkgs.callPackage ./package.nix { };

  enabledUsers = filterAttrs (_: u: u.enable) cfg.users;
  stateDir = "/var/lib/manzil";

  activationFor = name: userCfg:
    let new = mkManifest name userCfg; in ''
      _old=${stateDir}/manifest-${name}.json
      if ${pkgs.util-linux}/bin/runuser -u ${name} -- \
           ${linker}/bin/manzil ${new} "$_old"; then
        install -m 0644 ${new} "$_old"
      else
        echo "manzil: linker failed for ${name}; state not updated" >&2
      fi
    '';
in
{
  options.manzil = {
    clobberByDefault = mkOption {
      type = bool;
      default = false;
      description = "Default `clobber` value for every file, every user.";
    };

    users = mkOption {
      type = attrsOf userType;
      default = { };
      description = "Per-user configurations, keyed by username.";
    };
  };

  config = mkIf (enabledUsers != { }) {
    # Each enabled file must produce a source. `text` populates `source`
    # via mkDefault, so the only failure mode left is "neither set".
    assertions = flatten (mapAttrsToList (name: u:
      map (f: {
        assertion = !f.enable || f.source != null;
        message   = "manzil.users.${name}.files.\"${f.target}\": "
                  + "either `text` or `source` must be set.";
      }) (attrValues u.files)
    ) enabledUsers);

    system.activationScripts.manzil = stringAfter [ "users" "groups" ] ''
      install -d -m 0755 ${stateDir}
      ${concatMapStringsSep "\n" (n: activationFor n enabledUsers.${n})
        (attrNames enabledUsers)}
    '';

    environment.systemPackages = [ linker ];
  };
}
