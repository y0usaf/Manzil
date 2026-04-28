# manzil

> منزل — *home* (ar.)

A deliberately tiny replacement for Home Manager's `home.files`. Two files of
logic — a NixOS module and a bash linker — and that's the entire project.

## Scope

manzil only does what `home.files` does, no more:

- Declare files in a user's `$HOME` from Nix.
- Link them with symlinks to `/nix/store`.
- Track them across rebuilds via a JSON manifest, so removed entries are
  cleaned up (no dangling symlinks).

It does **not** ship XDG helpers, generators, format writers, systemd unit
generation, mime-apps, environment variables, package management, or
per-platform abstractions. Layer those on top yourself if you want them.

## Usage

```nix
# flake.nix
{
  inputs.manzil.url = "github:you/manzil";
  inputs.nixpkgs.url = "...";

  outputs = { nixpkgs, manzil, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        manzil.nixosModules.default
        ({ pkgs, ... }: {
          users.users.alice = { isNormalUser = true; home = "/home/alice"; };

          manzil.users.alice.files = {
            ".bashrc".text = ''
              alias ll='ls -la'
            '';

            ".config/foo".source = ./dotfiles/foo;

            ".local/bin/script" = {
              source     = ./bin/script.sh;
              executable = true;
            };

            # Overwrite a pre-existing non-symlink file:
            ".gitconfig" = {
              source  = ./gitconfig;
              clobber = true;
            };
          };
        })
      ];
    };
  };
}
```

## Options

| Option                                              | Type        | Default                       | Notes                                            |
| --------------------------------------------------- | ----------- | ----------------------------- | ------------------------------------------------ |
| `manzil.clobberByDefault`                           | bool        | `false`                       | Default `clobber` for every file.                |
| `manzil.users.<name>.enable`                        | bool        | `true`                        | Skip the user's files when `false`.              |
| `manzil.users.<name>.directory`                     | path        | `users.users.<name>.home`     | Root for relative file targets.                  |
| `manzil.users.<name>.clobberByDefault`              | bool        | `manzil.clobberByDefault`     | Per-user override.                               |
| `manzil.users.<name>.files."path".text`             | str?        | `null`                        | Inline file contents. Mutually exclusive w/ `source`. |
| `manzil.users.<name>.files."path".source`           | path?       | `null` (or text-derived)      | Source path to symlink to.                       |
| `manzil.users.<name>.files."path".target`           | str         | the attribute name            | Path relative to `directory`.                    |
| `manzil.users.<name>.files."path".executable`       | bool        | `false`                       | Apply +x (only with `text`).                     |
| `manzil.users.<name>.files."path".clobber`          | bool        | `clobberByDefault`            | Overwrite existing non-symlink target.           |
| `manzil.users.<name>.files."path".enable`           | bool        | `true`                        | Skip the entry when `false`.                     |

## How it works

1. The module renders one JSON manifest per user (`/nix/store/...-manzil-manifest-<user>.json`).
2. A NixOS activation script, ordered after `users`/`groups`:
   - Runs the linker as the target user via `runuser`.
   - Linker compares the new manifest with the previous one in `/var/lib/manzil/`
     and reconciles symlinks: prunes removed entries, links new ones, refreshes
     changed ones.
   - Copies the new manifest into `/var/lib/manzil/manifest-<user>.json` for
     the next rebuild.

### Manifest schema

```json
{
  "files": [
    { "target": "/home/alice/.bashrc",     "source": "/nix/store/...", "clobber": false },
    { "target": "/home/alice/.gitconfig",  "source": "/nix/store/...", "clobber": true  }
  ]
}
```

### Linker rules

- Target in old manifest, not in new → removed iff it's still a symlink.
  User-replaced regular files are left alone (with a warning).
- Target in new manifest:
  - Existing symlink → atomically updated with `ln -sfn`.
  - Existing non-symlink + `clobber=true`  → removed and replaced.
  - Existing non-symlink + `clobber=false` → warned, skipped.
  - Missing → created.

You can invoke the linker directly:

```sh
manzil-link /path/to/new.json /var/lib/manzil/manifest-alice.json
```

## Non-goals

- Multi-platform (Darwin, etc.). NixOS only.
- Reload/restart of user services on file change — out of scope; wire it up
  yourself with `systemd.user.services.<x>.restartTriggers` if you need it.
- Per-file uid/gid/perms beyond `executable` — symlinks don't carry mode, and
  the user owns their own home.

## License

MIT, or whatever you like — the whole thing is ~150 lines.
