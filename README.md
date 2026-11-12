# Stagehand

Installs, updates and starts the show tools: [Show Master](https://github.com/Cheter0410/Show-Master),
Beamweave and whatever comes next. Written in Rust (egui), for macOS, Windows and Linux.

- One card per app: installed version, newest release, **Install**, **Update**, **Open**, and what's new.
- Downloads come from each app's GitHub releases and are checked against the SHA-256 GitHub lists.
- A running app is never replaced (a show may be going on).
- Stagehand updates itself from `Cheter0410/stagehand-releases`.

Where apps go:

| System  | Release file                    | Installed to                                     |
|---------|---------------------------------|--------------------------------------------------|
| macOS   | `.dmg`                          | `/Applications` (or `~/Applications`)            |
| Windows | `…-Setup-x.y.z.exe` (Inno)      | where the installer puts it (silent, per user)   |
| Linux   | `…-linux-x86_64.tar.gz`         | `~/.local/share/stagehand/apps`, menu entry, `~/.local/bin` |

On Linux an app installed by the system (e.g. the Arch package of Show Master) is found on `PATH`.

## Adding an app

Add an entry to `apps.json` (name, tagline, colour, releases repository, release file names per system,
the Inno Setup `AppId`) and copy the same file to the default branch of `Cheter0410/stagehand-releases`:
installed copies read the catalog from there, so the new app appears without a new Stagehand. An app
that is not built in needs an `icon_url` (PNG).

## Build and install

Linux:

    ./install.sh

Mac (into `~/Desktop/dev/stagehand`):

    bash install-mac.sh

## Release

1. Raise `version` in `Cargo.toml`, add `## x.y.z` to `CHANGELOG.md`, commit, push.
2. Arch PC: `./packaging/build-on-linux.sh` (Linux + Windows; Windows needs MinGW, Wine and Inno Setup,
   the same set-up as Show Master's `docs/release-anleitung.md`).
3. Mac: `bash install-mac.sh`.

Both ask at the end whether to upload to the release (`gh` logged in; `PUBLISH=1` without asking).

The app icon is `assets/icon.svg`; `node scripts/render-icon.mjs` renders the PNG and ICO from it.
