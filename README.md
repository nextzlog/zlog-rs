# zLog Max

![image](https://img.shields.io/badge/Rust-2021-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)

The contest logger zLog in Rust for macOS, Windows and Linux, based on [jr8ppg/zLog](https://github.com/jr8ppg/zLog).
Every contest, file format, window and device is a crate linked at build time into one program.

## Architecture

```mermaid
flowchart TD
  App[apps/zlog-max] --> Ext[extensions]
  Ext --> Drv[drivers]
  Drv --> Core[core]
  Drv --> Haml[haml]
  Ext --> Modems[rtty, ftxl, cw4i]
```

| Layer | Contents |
|---|---|
| core | interfaces (zlog-core, zlog-ui), hosts (zlog-gui, zlog-cli), standard window parts (zlog-std) |
| drivers | rigs, keyers, SO2R, serial ports, remote, sound |
| extensions | contest rules, log formats, windows, digital modes, network services |

Dependencies point from extensions to drivers to core.
The test layers.rs of apps/zlog-max checks this from the folders.

Each crate has a `register` function that adds its parts to one `zlog_ui::Builder`.

```mermaid
flowchart LR
  Main[zlog_gui::run] --> App[zlog_max::register]
  App --> Builder
  Crates[each crate] -->|register| Builder
```

### Log Changes

Every change to the log is a command, and every view of it is read only.

```mermaid
flowchart LR
  Op[operator or extension] -->|command| Book
  Book -->|score| Contest
  Book -->|event| Observers
  Observers -->|queued command| Book
  Book -->|view| Panels
  Panels -->|entry action| Op
```

### Menus and Options

The menu bar, the toolbar and the shortcuts show one list of commands.
Each Options tab writes its own section of settings.toml.

```mermaid
flowchart LR
  Commands --> Menu[menu bar]
  Commands --> Toolbar
  Commands --> Keys[shortcuts]
  Options --> Toml[(settings.toml)]
```

### Devices and Band Scope

The cluster and the decoders send spots to the band scope.
The RTTY and FT modems key the rig through one PTT hub and follow its dial.

```mermaid
flowchart LR
  Cluster -->|spot| Scope[band scope]
  Decoders -->|spot| Scope
  Modems -->|PTT| Hub[PTT hub]
  Hub --> Rig
  Rig -->|dial| Modems
```

### Logs and Contests

| Crate | Role |
|---|---|
| zlog-qxsl | ADIF, Cabrillo, CTESTWIN, HAMLOG, JARL, QXML and zLog text through rxsl |
| zlog-summary | JARL E-Log and Cabrillo summary sheets |
| zlog-jarl | JARL Web submission |
| zlog-cfg | CFG and DAT contest definitions |
| zlog-lua | Luau scripts in a helper process |
| zlog-zylo | ZyLO plugins in a helper process |

The Lua API is in [crates/extensions/zlog-lua](crates/extensions/zlog-lua/README.md).

## Download

```mermaid
flowchart LR
  Nightly[nightly release] --> Mac["macOS: zlog-max.dmg, zlog-max-cli-macos"]
  Nightly --> Win["Windows: zlog-max.exe, zlog-max-windows.zip, zlog-max-cli.exe"]
  Nightly --> Linux["Linux: zlog-max-x86_64.AppImage, zlog-max-cli-linux"]
  Nightly --> Sha[SHA-256 of each build, THIRD-PARTY-NOTICES.html]
  Update["Help > Check for updates"] -->|newer nightly, settings kept| Nightly
```

Builds of master are on the [nightly release](https://github.com/nextzlog/zmax/releases/tag/nightly).
On Linux, run `chmod +x` on the AppImage.
For serial devices on Linux, add the user to the `dialout` group.
The manual is at [use.zlog.org](https://use.zlog.org).

## Build

```sh
$ cargo build --release -p zlog-max
$ ./target/release/zlog-max
```

```mermaid
flowchart LR
  Root[Cargo.toml: one workspace] --> Apps["apps: zlog-max, zlog-testing"]
  Root --> Crates["crates: core, drivers, extensions"]
  Crates --> Crate["zlog-*: src, lang (en.lang, ja.lang), data, tests, tools"]
```

## Test

```sh
$ cargo test --workspace
$ cargo test --release -p zlog-max -- --ignored
```

```mermaid
flowchart LR
  Unit["cargo test --workspace"] --> UI["-p zlog-max -- --ignored: headless click-through tests"]
  UI --> Shared[zlog-testing: GUI test helpers]
  UI --> CoreOnly["core_*.rs: the core crates alone"]
  Unit --> Regress["zlog regress DIR: outputs byte-exact against tests/regress/*/expected"]
```

## Release

```mermaid
flowchart LR
  Push[push to master] --> CI[".github/workflows/build.yaml"]
  CI --> Test["scripts/test.sh: fmt, clippy, tests, GUI tests"]
  Test --> Matrix["scripts/release.sh on ubuntu-22.04, macos-latest, windows-latest"]
  Matrix --> Pack[scripts/macos.sh and scripts/linux.sh]
  Pack --> Publish["scripts/publish.sh: nightly release"]
```

## License

### Author

[無線部開発班](https://nextzlog.dev)

### Clauses

- [MIT License](LICENSE).
- Toolbar icons: [Phosphor Icons](https://phosphoricons.com) (MIT).
- 出典: 国土交通省国土数値情報
- Map background: © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors.
- Other licences: `THIRD-PARTY-NOTICES.html` in each release.
