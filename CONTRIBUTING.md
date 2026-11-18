# Contributing

Thanks for your interest! Bug reports and pull requests are welcome.

## Code of Conduct

All participation implies agreement with
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Bug reports

Include: version (Settings → О программе), OS, what was done, what happened,
expected result, and relevant log lines from `%LOCALAPPDATA%\ChipClean`
(`chipclean.log`, `operations.jsonl`). Do not attach registry dumps or
personal files.

## Development

Requirements: Rust 1.95+, Windows 10/11.

Checks a change must pass:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Run the full set before pushing. AI-assisted changes are welcome but must be
reviewed by a human before merge (branch protection requires a pull request).

## Project structure

- `src/core` — logic: cleaner, registry, startup, privacy, community, tweaks,
  disk analysis, rollback, scheduling, shell integration.
- `src/ui` — egui pages.
- `src/i18n.rs` — ru/en localization.
- `src/core/tweaks_data.rs` — generated tweak catalog (from Reg Organizer's
  open config; keep the generator reproducible).

## Commit style

- Conventional Commits: `feat`, `fix`, `refactor`, `style`, `docs`, `ci`,
  `chore`.
- Keep commits thematic and buildable.

## Release process

See [RELEASING.md](RELEASING.md).
