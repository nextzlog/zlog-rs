zlog-rs
====

![image](https://img.shields.io/badge/Rust-2021-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)

zlog-rs is the contest logger [zLog](https://github.com/jr8ppg/zLog) rewritten in Rust for macOS and Windows.
Its core is small, and every contest, file format, window and device is an extension crate linked in at build time.

## Architecture

The core knows no contest, no screen and no device; everything else is an extension crate, and the application links the ones its build includes.

```mermaid
flowchart TD
  subgraph App[Applications]
    GUI[zlog-gui]
    CLI[zlog-cli]
  end
  subgraph Ext[Extension crates]
    Rules[Rules: zlog-jarl, zlog-japan, zlog-world, zlog-general]
    Formats[Formats: zlog-qxsl]
    Observers[Observers: zlog-prefix]
    Panels[Panels: zlog-score, zlog-maplot, zlog-es, zlog-soumu, zlog-comports, zlog-latest]
    Devices[Devices: zlog-rig-cat, zlog-rig-sim]
    Modems[Modems: zlog-cw]
  end
  subgraph Api[Interfaces]
    Core[zlog-core: QSO, log, ZLO/ZLOX, book, extension traits]
    UI[zlog-ui: panels, settings pages]
    Rig[zlog-rig: rigs, transports, protocols]
    Modem[zlog-modem: decoders, encoders]
  end
  GUI --> Ext
  CLI --> Rules
  CLI --> Formats
  Rules --> Core
  Formats --> Core
  Formats --> QxSL[qxsl-rs]
  Observers --> Core
  Panels --> UI
  Devices --> Rig
  Modems --> Modem
  Modems --> Rumo[rumo]
  UI --> Core
  Rig --> Core
  Modem --> Core
```

```mermaid
sequenceDiagram
  participant File as ZLO or ZLOX file
  participant Op as Operator or extension
  participant Book
  participant Contest
  participant Ext as Observers and links
  participant Panels
  File->>Book: open
  Book->>Book: give QSOs without an ID one
  Book->>Contest: score the whole log, read only
  Contest-->>Book: dupes, points, multipliers, score sheet
  Book->>Ext: Opened, with every QSO of the file
  Op->>Book: command (insert, update, delete by QSO ID)
  Book->>Book: apply
  Book->>Contest: score the whole log, read only
  Contest-->>Book: dupes, points, multipliers, score sheet
  Book->>Ext: event with its origin
  Ext-->>Book: more commands, queued and run afterwards
  Note over Book,Ext: a chain of commands that does not settle is stopped, naming the extension
  Book-->>Panels: read-only view every frame
  Panels-->>Op: entry actions, such as a decoded callsign
```

Rig control keeps the rig and the entry row in step both ways, with the same code for a real rig and the built-in simulator.

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

## Extensions

| Kind | Trait | Provides |
|---|---|---|
| Rules | `ContestDef`, `Contest` | A contest in the zLog Menu: sent number, number check, scoring, multiplier table. |
| Formats | `LogFormat` | Import and export of a file format. |
| Observers | `Observer` | Reactions to changes of the log. |
| Links | `Link` | An observer that also receives from outside, such as a network. |
| Panels | `zlog_ui::Panel` | A window of its own, listed in the Windows menu. |
| Settings | `zlog_ui::SettingsPage` | A page of the Options dialog over the extension's own section. |
| Devices | `zlog_rig::Protocol`, `zlog_rig::Transport` | Rig control. |
| Modems | `zlog_modem::Demodulator`, `zlog_modem::Modulator` | Decoding and sending CW and other modes. |

An extension crate exports one `Extension` bundling its rules, formats, observers and links, and the application lists the crates it links in `registry.rs`.

## Build

```sh
$ cargo build --release -p zlog-gui
$ ./target/release/zlog-gui
```

Each extension is a Cargo feature of `zlog-gui`, so a build with fewer extensions is chosen with `--no-default-features --features ...`.

## Test

```sh
$ cargo test --workspace
```

## License

### Author

[無線部開発班](https://nextzlog.dev)

### Clauses

- This software is released under the [MIT License](LICENSE).
- Data and code taken from other projects keep their own notices in the crates that use them.
