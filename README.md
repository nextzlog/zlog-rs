# zLog Max

![image](https://img.shields.io/badge/Rust-2021-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)

The contest logger zLog in Rust for macOS, Windows and Linux, based on [jr8ppg/zLog](https://github.com/jr8ppg/zLog).
Every contest, file format, window and device is a crate linked at build time into one program.

## Architecture

```mermaid
flowchart TD
  App["apps/zlog-max: zlog-max and zlog binaries, register"] --> Ext
  App --> Drv
  App --> Core
  subgraph Ext[crates/extensions]
    Rules[Rules: zlog-rule-ja, zlog-rule-dx, zlog-cfg, zlog-lua]
    Formats[Formats and settings files: zlog-qxsl, zlog-summary, zlog-ini, zlog-toml, zlog-backup]
    Windows[Windows: zlog-edit, zlog-score, zlog-check, zlog-rate, zlog-scope, zlog-map, zlog-country, zlog-qsy, zlog-op, zlog-history, zlog-search, zlog-guide, zlog-after, zlog-form, zlog-number]
    Modes[Modes: zlog-rtty, zlog-ft8, zlog-cw4isr]
    Network[Network: zlog-cluster, zlog-link, zlog-cty, zlog-zylo, zlog-webhook, zlog-jarl, zlog-license, zlog-ja6ycu, zlog-es, zlog-market]
  end
  subgraph Drv[crates/drivers]
    Rig[Rigs: zlog-rig]
    Keyers[Keyers: zlog-keyer, zlog-keyer-line, zlog-keyer-sim, zlog-keyer-winkey, zlog-keyer-if4cw]
    So2r[SO2R: zlog-so2r, zlog-so2r-neo, zlog-so2r-otrsp]
    Ports[Ports and remote: zlog-comport, zlog-remote]
    Sound[Sound: zlog-audio, zlog-voice]
  end
  subgraph Core[crates/core]
    Api[Interfaces: zlog-core, zlog-ui, zlog-serial, zlog-net, zlog-process, zlog-macro]
    Host[Host: zlog-gui, zlog-cli]
    Std[zlog-std: register of the window parts, zlog-rule, zlog-open, zlog-entry, zlog-menu, zlog-mac, zlog-toolbar, zlog-status, zlog-table, zlog-dock, zlog-style, zlog-bind, zlog-undo, zlog-update]
  end
  Ext --> Drv --> Core
  Drv --> Haml[haml]
  Modes --> Modems[rtty, ftxl, cw4i]
  Modes -->|PTT through zlog_ui::tx| Haml
```

Dependencies point from extensions to drivers to core; the test layers.rs of apps/zlog-max checks this from the folders.

```mermaid
flowchart LR
  Crate[each crate] -->|"register(b: &mut Builder)"| Builder["zlog_ui::Builder"]
  App["zlog_max::register"] -->|"zlog_std::register, then every crate"| Builder
  Main["zlog_gui::run(zlog_max::register)"] --> Builder
```

### Extensions

```mermaid
flowchart LR
  subgraph CoreApi[zlog-core]
    Extension[Extension]
    Extension --> ContestDef[ContestDef, Contest: rules and scoring]
    Extension --> ContestFile[ContestFile: contests read from files]
    Extension --> LogFormat[LogFormat: import and export]
    Extension --> Observer[Observer: on_event per change, on_step per operator action]
    Extension --> Link[Link: an observer polled, may resync the log]
  end
  subgraph UI[zlog-ui]
    Panel[Panel: a window, its commands, keys and settings]
    Panel --> Actions[EntryAction: edit, say, transmit, run, spot]
    SettingsPage[SettingsPage: a tab of Options]
    StartScreen[StartScreen: the screen until a log is open]
    EntryStyle[EntryStyle: draws the entry, its keys]
    Driver[device: DeviceDriver, Drivers, PortHub, AudioDrv, ModemDrv]
    Chrome[chrome: MenuBar, ToolBar, StatusBar, LogTable, WindowHost]
  end
```

### Log Changes

```mermaid
sequenceDiagram
  participant File as ZLO or ZLOX file
  participant Op as Operator or extension
  participant Book
  participant Contest
  participant Ext as Observers and links
  participant Panels
  File->>Book: open
  Book->>Contest: score the whole log, read only
  Op->>Book: command (insert, update, delete, rescore)
  Book->>Contest: score the whole log, read only
  Book->>Ext: event with its origin
  Ext-->>Book: more commands, queued and run afterwards
  Book-->>Panels: read-only view every frame
  Panels-->>Op: entry actions, such as a decoded callsign
```

### Menus, Toolbar and Options

```mermaid
flowchart LR
  AppCmds[File, Edit, Settings commands] --> List[one list of commands]
  PanelCmds[every panel's commands] --> List
  List --> MenuBar[zlog-menu: menu bar; zlog-mac on macOS]
  List --> Toolbar[zlog-toolbar: toolbar]
  List --> BindTab[zlog-bind: shortcuts and toolbar tabs]
  Tabs[Options tabs of the core and every extension] -->|OK writes their sections| Toml[(settings.toml)]
  Docking[zlog-dock: tool windows docked left, right or bottom] --> Toml
```

### Devices and Band Scope

```mermaid
flowchart LR
  Menu[Devices menu] -->|one submenu per device| DevWindows[rig, keyer, voice, RTTY, SO2R windows]
  Options[Options tabs] -->|device, port, timing, messages| DevWindows
  Cluster[zlog-cluster] -->|EntryAction::Spot| Spots[view.spots]
  Decoders[zlog-rtty, zlog-cw4isr, zlog-ft8] -->|EntryAction::Spot| Spots
  Spots --> Scope[zlog-scope]
  Modems[RTTY and FT engines] -->|Ptt| Hub[zlog_ui::tx TxHub, haml Arbiter]
  Hub -->|Keyed, KeyFailed, PttLead| Modems
  RigDial[zlog-rig] -->|Dial| Modems
  Modems -->|FT Tune as EntryAction::Qsy| RigDial
```

### Logs, Contests and Plugins

```mermaid
flowchart LR
  Book[Book] <-->|own log files| Zlo[(ZLO, ZLOX)]
  Book <-->|zlog-qxsl, rxsl| Formats[(ADIF, Cabrillo, CTESTWIN, HAMLOG, JARL, QXML, zLog text)]
  Book --> Summary[zlog-summary: JARL E-Log, Cabrillo through rxsl sheets] --> Jarl[zlog-jarl: JARL Web submission]
  Cfg[(CFG and DAT)] --> CfgCrate[zlog-cfg] --> Lua[zlog-lua: Luau in a helper process]
  Lists[zlog-market, zlog-ja6ycu] --> CfgCrate
  Book --> Zylo[zlog-zylo: ZyLO plugins in a helper process]
```

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
  Root[Cargo.toml: one workspace, one Cargo.lock] --> Apps["apps: zlog-max, zlog-testing"]
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
