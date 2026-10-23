zLog RS
====

![image](https://img.shields.io/badge/Rust-2021-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)

zLog RS is the contest logger zLog rewritten in Rust for macOS and Windows, based on [jr8ppg/zLog](https://github.com/jr8ppg/zLog).
Its core is small, and every contest, file format, window and device is an extension crate linked in at build time.

## Architecture

```mermaid
flowchart TD
  subgraph App[Applications]
    GUI[zlog-gui]
    CLI[zlog-cli]
  end
  subgraph Ext[Extension crates]
    Rules[Rules: zlog-rule, zlog-rule-ja, zlog-rule-dx, zlog-cfg]
    Formats[Formats: zlog-qxsl]
    Panels[Panels: zlog-undo, zlog-edit, zlog-score, zlog-check, zlog-rate, zlog-summary, zlog-scope, zlog-map, zlog-es, zlog-soumu, zlog-ports, zlog-update, zlog-qth, zlog-qsy, zlog-ops]
    Network[Network: zlog-cluster, zlog-link, zlog-dc, zlog-jarl]
    Devices[Devices: zlog-rig-cat, zlog-rig-sim, zlog-keyer, zlog-so2r]
    Modems[Modems: zlog-cw, zlog-rtty]
    Start[zLog Menu: zlog-open, zlog-cfg]
    Entry[Entry styles: zlog-entry]
    Style[Look: zlog-style]
    Bind[Shortcuts: zlog-bind]
    Lists[Contest lists: zlog-ja6ycu]
    Plugins[ZyLO extensions: zlog-zylo]
    Ini[zlog.ini: zlog-ini]
  end
  subgraph Api[Interfaces]
    Core[zlog-core: QSO, log, ZLO/ZLOX, book, extension traits]
    UI[zlog-ui: panels, commands, menu sections, shortcuts, status bar, settings tabs, dialogs, palette, start screen, entry styles, contest menu entries, contest lists]
    Rig[zlog-rig: rigs, transports, protocols]
    Net[zlog-net: downloads, posts, form submissions, offline mode]
  end
  GUI --> Ext
  CLI --> Rules
  CLI --> Formats
  Rules --> Core
  Formats --> Core
  Formats --> QxSL[qxsl-rs]
  Panels --> UI
  Network --> UI
  Network --> Net
  Lists --> UI
  Start --> UI
  Entry --> UI
  Style --> UI
  Bind --> UI
  Plugins --> UI
  Plugins --> Rules
  Devices --> Rig
  Modems --> UI
  Modems --> Cw4i[cw4i]
  Modems --> Devices
  Cw4i --> Rumo[rumo]
  Lists --> Net
  UI --> Core
  Rig --> Core
```

## Extensions

```mermaid
flowchart LR
  subgraph Core[zlog-core]
    Extension[Extension]
    Extension --> ContestDef[ContestDef, Contest: rules and scoring]
    Extension --> ContestFile[ContestFile: contests read from files]
    Extension --> LogFormat[LogFormat: import and export]
    Extension --> Observer[Observer: on_event per change, on_step per operator action]
    Extension --> Link[Link: an observer polled, may resync the log]
    Book[Book: execute, begin_step and end_step, execute_if a Batch]
  end
  subgraph UI[zlog-ui]
    Panel[Panel: window or none, commands, status, on_key, tick, dialogs, editing, load and save]
    Panel --> Actions[EntryAction: entry row, Edit a Batch, Say, Transmit, KeyerPort]
    Commands[Commands, Command: menus, toolbar, shortcut, row_key, section and weight]
    Section[MenuSection: the order of a menu, separators between sections]
    SettingsPage[SettingsPage: a tab of Options, reloads the panel of its section, may assign shortcuts]
    ContestList[ContestList: contests published elsewhere]
    StartScreen[StartScreen: the screen until a log is open]
    EntryStyle[EntryStyle: draws the entry, its keys]
    EntryStyle --> EntryState[EntryState: fields, log, dupe check, band, mode, power cycles]
    Palette[palette: colours of meaning for light and dark]
    Queue[queue: the SO2R play queue of keyer and voice messages]
    ContestEntry[ContestEntry: an entry of its contest menu]
    Dialog[dialog: windows, pinned buttons, ok_cancel, confirm_row, file pickers]
  end
  subgraph Net[zlog-net]
    Job[Job: work on a thread, polled; Fetch, Exchange of a Request]
  end
  ContestDef --- RuleCrates[zlog-rule, zlog-rule-ja, zlog-rule-dx]
  ContestFile --- Cfg[zlog-cfg, zlog-zylo]
  LogFormat --- Qxsl[zlog-qxsl, zlog-zylo]
  Link --- LinkCrates[zlog-link, zlog-zylo]
  Observer --- ObserverCrates[zlog-dc, zlog-undo]
  Panel --- PanelCrates[every window crate, zlog-undo menu, zlog-edit dialogs]
  SettingsPage --- Tabs[station, preferences, rig, toolbar, zlog-bind, zlog-style, zlog-zylo, zlog-dc]
  ContestList --- Ja6ycu[zlog-ja6ycu]
  StartScreen --- Open[zlog-open]
  EntryStyle --- EntryCrates[zlog-entry]
  ContestEntry --- Entries[contest groups of ContestDef, zlog-cfg window]
  Job --- NetCrates[zlog-dc, zlog-jarl, zlog-cfg, zlog-update, zlog-soumu, zlog-ja6ycu]
```

### Log changes

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
  Book->>Ext: Opened, with every QSO of the file
  Op->>Book: command (insert, update, delete, rescore)
  Book->>Book: apply
  Book->>Contest: score the whole log, read only
  Book->>Ext: event with its origin
  Ext-->>Book: more commands, queued and run afterwards
  Note over Book,Ext: a chain of commands that does not settle is stopped, naming the extension
  Book-->>Panels: read-only view every frame
  Panels-->>Op: entry actions, such as a decoded callsign
```

### Menus, toolbar and Options

```mermaid
flowchart LR
  AppCmds[File, Edit, Device commands] --> List[one list of commands]
  PanelCmds[every panel's commands] --> List
  List -->|by section, weight, label| Menu[menu bar]
  List --> Toolbar[toolbar]
  List --> ToolbarTab[toolbar tab: shown, order, style]
  List --> BindTab[zlog-bind tab: shortcuts]
  BindTab -->|shortcuts chosen| List
  OwnTabs[station, preferences, rig, toolbar] --> Tabs[Options tabs]
  ExtTabs[extension tabs] --> Tabs
  Tabs -->|OK writes their sections| Toml[(settings.toml)]
  OldIni[(settings.ini of earlier versions)] -->|first start| Toml
  Toml <-->|zlog-ini, section by section| ZlogIni[(zlog.ini, Shift_JIS)]
```

### Rig control

```mermaid
sequenceDiagram
  participant Entry as Entry row
  participant Sync as Rig sync
  participant CAT as CAT protocol
  participant Port as Serial port or simulator
  Entry->>Sync: band or mode chosen
  Sync->>CAT: last frequency or band edge, and the mode
  CAT->>Port: command bytes
  Port-->>CAT: frequency and mode reports
  CAT-->>Sync: rig state
  Sync-->>Entry: band and mode follow the rig
```

### Logs and submission files

```mermaid
flowchart LR
  Book[Book] <-->|own log files| Zlo[(ZLO, ZLOX)]
  Book -->|File > Export| Qxsl[zlog-qxsl]
  Qxsl -->|File > Import| Book
  Qxsl <--> Formats[(ADIF, Cabrillo, CTESTWIN, HAMLOG, JARL, QXML, zLog text)]
  Book --> Summary[zlog-summary]
  Summary -->|header and checks| Sheets[(JARL E-Log, Cabrillo)]
  Qxsl -->|QSO lines| Sheets
```

### User-defined contests

```mermaid
flowchart LR
  Lists[contest lists such as zlog-ja6ycu] -->|contests, files, rules, changes| Window[user-defined contest window]
  Folder[(contest folder)] --> Window
  Window -->|Download, zlog-net| Folder
  Import[Import or drop, from anywhere] --> Folder
  Window -->|OK| Cfg[zlog-cfg: CFG and DAT]
  Groups[contest groups of the registry] -->|ContestEntry| Menu[zLog Menu: zlog-open]
  Window -->|ContestEntry, zlog-cfg window| Menu
  Menu -->|Setup| App[zlog-gui opens the log]
```

### ZyLO extensions

```mermaid
sequenceDiagram
  participant App as zLog RS
  participant Helper as helper process (zlog-rs zylo-host)
  participant Lib as shared library (.dll, .dylib, .so)
  App->>Helper: start in the background when enabled
  Helper->>Lib: load, check the ZyLO exports
  App->>Helper: log events, queued while starting
  Lib-->>Helper: callbacks (insert, update, query)
  Helper-->>App: commands for the book
  App->>Helper: finish, killed after a grace
```

### Updates

```mermaid
sequenceDiagram
  participant App as zLog RS
  participant GitHub as nightly release
  App->>GitHub: Help > Update
  GitHub-->>App: build of the platform and its SHA-256
  App->>App: check, replace the program, offer a restart
```

### Crate layout

```mermaid
flowchart LR
  Crate[crates/zlog-*] --> Src[src: Rust code]
  Crate --> Data[data: embedded data]
  Crate --> Lang[lang: en.lang, ja.lang]
  Crate --> Tests[tests: integration tests and fixtures]
  Crate --> Tools[tools: scripts that regenerate data]
```

## Download

Builds of master for macOS and Windows are on the [nightly release](https://github.com/nextzlog/zlog-rs/releases/tag/nightly).
The manual is at [use.zlog.org](https://use.zlog.org).

## Build

```sh
$ cargo build --release -p zlog-gui
$ ./target/release/zlog-gui
```

Each extension is a Cargo feature of `zlog-gui`.

## Test

```sh
$ cargo test --workspace
```

## License

### Author

[無線部開発班](https://nextzlog.dev)

### Clauses

- This software is released under the [MIT License](LICENSE).
- The toolbar icons are [Phosphor Icons](https://phosphoricons.com) (MIT, Copyright (c) 2023 Phosphor Icons), embedded in zlog-ui with codepoints from [egui-phosphor](https://github.com/amPerl/egui-phosphor) (MIT OR Apache-2.0, Copyright (c) 2023 Romet Tagobert); see [crates/zlog-ui/LICENSE](crates/zlog-ui/LICENSE).
