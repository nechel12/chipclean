# ChipClean

[![CI](https://github.com/nechel12/chipclean/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/nechel12/chipclean/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/nechel12/chipclean)](https://github.com/nechel12/chipclean/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/nechel12/chipclean/total)](https://github.com/nechel12/chipclean/releases)
[![License](https://img.shields.io/github/license/nechel12/chipclean)](LICENSE)

Fast, safe Windows cleaner and registry optimizer — written in Rust, inspired
by Reg Organizer. Everything runs locally: no telemetry, no cloud uploads.

> Status: pre-1.0. Settings and internal formats may change in minor releases;
> changes are listed in [CHANGELOG.md](CHANGELOG.md).

## Features

| Module | What it does |
|---|---|
| Очистка системы | Temporary files, caches, logs, error reports, Windows updates, Recycle Bin, downloads. |
| Реестр | Safe check, editor with search/replace, `.reg` import with preview, snapshots, rollback. |
| Автозагрузка | Run/RunOnce, startup folders, Task Scheduler tasks, services — disable with quarantine. |
| Личные данные | Browser history, recent documents, file lists. |
| Чистка сообществом | winapp2.ini rules with online database update. |
| Твики | Hidden Windows settings (51 tweaks) with `.reg` backups before each change. |
| Фоновая очистка | Daily task and logon check via Task Scheduler. |
| Экспресс-проверка | One button runs all scans. |
| Анализ диска | Largest files and volume usage. |
| Откат | Registry backups, quarantine restore, Windows restore point creation. |
| Интеграция | Context menu for `.reg` files («Предпросмотр в ChipClean»). |

## System requirements

- Windows 10/11 x64 (administrator rights for most operations)
- The app requests elevation (UAC) on start

## Install

Download the latest `chipclean.exe` from
[Releases](https://github.com/nechel12/chipclean/releases/latest). No installer
is required: put the file anywhere and run it. Verify the checksum against
`SHA256SUMS.txt` of the release:

```powershell
(Get-FileHash .\chipclean.exe).Hash
```

## Quick start

1. Run `chipclean.exe` (allow UAC).
2. Open «Очистка системы» → select categories → «Сканировать» → preview → confirm.
3. Use «Реестр» to inspect/fix registry problems; changes are backed up to `.reg`.
4. «Твики» lists hidden system settings; each change saves a `.reg` backup first.
5. «Настройки» → «Фоновая очистка» registers a daily task and a logon check.

## Background cleanup

The scheduler runs the same `chipclean.exe` with `--background-cleanup`.

## Build from source

[Rust](https://rustup.rs/) 1.95+:

```sh
cargo build --release
cargo test
cargo clippy --all-targets
cargo fmt --check
```

## Data and privacy

- All operations are written to a local operations journal.
- Tweak changes create `.reg` backups under the app data folder.
- The community rules database is downloaded **only** when you press the button;
  no local data is ever uploaded.

## Security

See [SECURITY.md](SECURITY.md) for reporting. Do not post registry data or
personal files in public issues.

## Related projects

- [Reg Organizer](https://www.chemtable.com/) — the original tool whose
  tweak catalog and behavior inspire ChipClean (data ported from the open
  config file, Apache-2.0 compatible direction).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md); participation implies agreement with
the [Code of Conduct](CODE_OF_CONDUCT.md). Release process and tag
verification are in [RELEASING.md](RELEASING.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
