# Changelog

All notable changes to **aadc** (ASCII Art Diagram Corrector) are documented here.

Repository: <https://github.com/Dicklesworthstone/aadc>

This project has not yet cut a formal release or tag. All entries below correspond
to commits on the `main` branch, grouped into logical phases of development.
Links point to individual commits on GitHub.

---

## [Unreleased] — v0.1.0-dev

Current version in `Cargo.toml`: **0.1.0**

### 2026-03-13 — Housekeeping

- **chore:** remove stale macOS resource fork file `._aadc_illustration.png`
  ([`57f6537`](https://github.com/Dicklesworthstone/aadc/commit/57f65375ad67212dd59c79b05dda949c6aa13e4d))

### 2026-02-21 .. 2026-02-25 — License and Documentation

- **docs:** add cass (Cross-Agent Session Search) tool reference to AGENTS.md
  ([`3bfd864`](https://github.com/Dicklesworthstone/aadc/commit/3bfd86426d591367c2d8450b8644516804bd901b))
- **docs:** update README license references to MIT + OpenAI/Anthropic Rider
  ([`bc5b8c4`](https://github.com/Dicklesworthstone/aadc/commit/bc5b8c46e4fef629dfb420c48ef950849af6ff05))
- **chore:** update license to MIT with OpenAI/Anthropic Rider
  ([`57df125`](https://github.com/Dicklesworthstone/aadc/commit/57df125085bbf3d4ea5b9b7d64768e55b2318dce))
- **chore:** add GitHub social preview image (1280x640)
  ([`474f4b4`](https://github.com/Dicklesworthstone/aadc/commit/474f4b4b76e13ed58311cd9a892408489c9d2449))

### 2026-02-15 — Dependency Stabilization

- **deps:** update `rich_rust` from pre-release git ref to crates.io v0.2.0, improving build
  reproducibility and eliminating git fetches during `cargo build`
  ([`74382fd`](https://github.com/Dicklesworthstone/aadc/commit/74382fd1c75720c4c16a66869dccec7679a481be))

### 2026-02-14 — Documentation

- **docs:** update AGENTS.md with latest multi-agent conventions
  ([`65ffbf2`](https://github.com/Dicklesworthstone/aadc/commit/65ffbf2cd0367d87ac085790bc32c909afd16d45))

### 2026-02-01 .. 2026-02-02 — Test Results

- **test:** update E2E test results with latest run
  ([`bd4255b`](https://github.com/Dicklesworthstone/aadc/commit/bd4255b2e8d1f71a4224132a431199f826f05213))
- **test:** update E2E test results log
  ([`4fd16e3`](https://github.com/Dicklesworthstone/aadc/commit/4fd16e3db47132dc41b16d2ed9ad709ee41cb364))

### 2026-01-25 — Bug Fixes, Installation, and CI Hardening

#### Bug Fixes

- **fix(test):** add `SafeOriginalDir` RAII guard for robust cwd handling in CI; on macOS
  CI the original working directory may become inaccessible, causing `current_dir()` to fail
  ([`6177d83`](https://github.com/Dicklesworthstone/aadc/commit/6177d834cbd65f05fd6a2315422bea146e5869ec))
- **fix(io):** preserve trailing newline when writing files; `join("\n")` was dropping
  the final newline, violating Unix text file conventions
  ([`d27071b`](https://github.com/Dicklesworthstone/aadc/commit/d27071b696998adc2006e07edf17a44b68974049))
- **fix(expand_tabs):** account for CJK character visual width in tab expansion;
  CJK double-width characters were being counted as width 1, causing incorrect
  tab stops after CJK content
  ([`72ecb61`](https://github.com/Dicklesworthstone/aadc/commit/72ecb613c8a3115887fbca48699dbcda0c49d6aa))

#### New: curl|bash Installer

- **feat:** add `install.sh` for one-line curl-based installation with platform
  auto-detection; enhance GitHub Actions release workflow with additional targets
  ([`cec424c`](https://github.com/Dicklesworthstone/aadc/commit/cec424c36d38f309c959ea7d034ae309e9fe7c1b))

#### Chore

- **chore:** add `/target/` to `.gitignore`
  ([`03dc4a9`](https://github.com/Dicklesworthstone/aadc/commit/03dc4a9463d7f86e5eb514925994f3f209a66f76))
- **chore:** apply `cargo fmt` and remove stale `dead_code` comment
  ([`6c4f1e9`](https://github.com/Dicklesworthstone/aadc/commit/6c4f1e951d4fdde3dde387428fd37dcc83391a1b))
- **chore:** parallel agent work — README and beads updates
  ([`654882f`](https://github.com/Dicklesworthstone/aadc/commit/654882fc9e37f41ebb4b8693c70f3b83ec51beca),
   [`bc0740d`](https://github.com/Dicklesworthstone/aadc/commit/bc0740d87a05e0f97a0b7f81f6ab30bda2db7396))

### 2026-01-24 — CI Stabilization Sprint

Six consecutive fixes to get GitHub Actions CI green on all platforms:

- **fix(ci):** run tests single-threaded (`--test-threads=1`) to avoid mutex poisoning
  ([`bf19fe2`](https://github.com/Dicklesworthstone/aadc/commit/bf19fe296b9dc101ecc2830d44587f8015dae784))
- **fix(ci):** build binary before running tests
  ([`636c0d9`](https://github.com/Dicklesworthstone/aadc/commit/636c0d963e2a45a1bcaa94995a528ccf953f9801))
- **fix(ci):** use cargo-provided binary path in integration tests
  ([`9b41ad5`](https://github.com/Dicklesworthstone/aadc/commit/9b41ad52f930a021391393592a4529e417afb74a))
- **fix(ci):** use git dependency for `rich_rust` (pre-crates.io publication)
  ([`b710f72`](https://github.com/Dicklesworthstone/aadc/commit/b710f7262f03336564f866185fa32e2b16df78c2))
- **fix(ci):** stabilize CWD tests and bench binary lookup
  ([`6dd17e6`](https://github.com/Dicklesworthstone/aadc/commit/6dd17e6e47df88e482d252e72b8cdc404993d975))
- **fix:** resolve clippy warnings in CI
  ([`8aac65f`](https://github.com/Dicklesworthstone/aadc/commit/8aac65fd2f824e365136351b3a64c10ced500dd0))

#### Chore

- **chore:** fix `cargo fmt` formatting
  ([`1bde9bf`](https://github.com/Dicklesworthstone/aadc/commit/1bde9bfa4daacc317ec29f05fc5a1194cd0f8f0d))
- **chore(gitignore):** add ephemeral file patterns for agent workflows, `a.out`
  ([`0f65064`](https://github.com/Dicklesworthstone/aadc/commit/0f65064b43c8bf49c923ffdefb618871943d6cbc),
   [`2f42a09`](https://github.com/Dicklesworthstone/aadc/commit/2f42a09232b8ff52d8deec0cf14ca74d5263d286))

### 2026-01-21 — Rapid Feature Build-Out

This is the primary feature development day. All major capabilities were added in a single
concentrated burst of development.

#### Core Features Added

- **feat:** add multiple file support — CLI accepts `Vec<PathBuf>`, shell globs work,
  continue-on-error with summary; semantic exit codes (0/1/2/3/4) infrastructure
  ([`6ce8aab`](https://github.com/Dicklesworthstone/aadc/commit/6ce8aab02817a36cfa0d91487255267f99ee3d07))
- **feat:** add confidence presets (`--preset/-P`: Strict/Normal/Aggressive/Relaxed),
  `--diff/-d` unified diff output, `--dry-run/-n` (exit 3 if changes), `--backup`
  before in-place edit, `--json` machine-readable output
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af))
- **feat:** add recursive directory mode (`-r/--recursive`) with `--glob`, `--max-depth`,
  and `.gitignore` awareness
  ([`2992e32`](https://github.com/Dicklesworthstone/aadc/commit/2992e32106a190f05bace63fd6967d822d71cdd1))
- **feat:** add config file system (`.aadcrc`), `--color` flag (auto/always/never),
  `aadc config init/show/path` subcommands, `rich_rust` terminal color detection
  ([`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee))
- **feat:** add `--lines/-L` flag for selective line range processing; supports
  `10-50`, `50-`, `-100`, `42`, and comma-separated ranges with automatic merging
  ([`0094fe2`](https://github.com/Dicklesworthstone/aadc/commit/0094fe26473929b1236903a46f0de2f07efe4575))
- **feat:** add statistics summary in verbose mode — timing, blocks found/processed/skipped,
  revisions applied/skipped, throughput (lines/sec); `Stats.merge()` for multi-file aggregation
  ([`078156f`](https://github.com/Dicklesworthstone/aadc/commit/078156f4f409b9f94e55025daabf0c213af7f996))

#### Testing and Quality

- **test:** add fixture-based test suite (`tests/e2e_fixtures.sh`), malformed-input edge
  case fixtures; streamline CI workflow
  ([`2dfb677`](https://github.com/Dicklesworthstone/aadc/commit/2dfb6776e7b774287c3b55d2b6022865e01b7bb2))
- **test:** implement comprehensive error handling (tab_width validation, 100 MB file size
  limit, high max_iters warning); add `tests/integration.rs` with 25 Rust integration tests,
  `tests/e2e_runner.sh` orchestrator; add `#![warn(missing_docs)]` and full doc coverage
  for public types. 164 tests passing (139 unit + 25 integration)
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8))
- **perf:** add `benches/correction.rs` Criterion benchmarks and `benches/benchmark.sh`
  ([`0066a4f`](https://github.com/Dicklesworthstone/aadc/commit/0066a4f1a443041a44cde469685c953e2bb9c037))

#### Architecture and Enhancements

- **feat:** enhance correction algorithms and improve benchmark scripts
  ([`adafd76`](https://github.com/Dicklesworthstone/aadc/commit/adafd7602f28ac7cb1eafe89a292c3e618da6f74))
- **refactor:** restructure `main.rs` for better modularity
  ([`28d920d`](https://github.com/Dicklesworthstone/aadc/commit/28d920df299819075ab0645443ad574d6ce74ad5))
- **feat:** enhance CLI functionality and documentation
  ([`c6f3892`](https://github.com/Dicklesworthstone/aadc/commit/c6f38924fbcd97ccfd51c063dc0554f1cc83db4f))
- **feat:** update Cargo config and main implementation
  ([`88e1244`](https://github.com/Dicklesworthstone/aadc/commit/88e124418828fc08cd7953c619316b1ecd4fe137))
- **chore:** iterative implementation refinements
  ([`52b0825`](https://github.com/Dicklesworthstone/aadc/commit/52b0825b46437ab87dd8a030e8ff46277a6e696c),
   [`16ad5c0`](https://github.com/Dicklesworthstone/aadc/commit/16ad5c0477929d289865b58bf00c6dbe16dff89d),
   [`9e91df2`](https://github.com/Dicklesworthstone/aadc/commit/9e91df21c3933599e0bd4c4c1a52d6b0e7f1f666))

#### Documentation and Licensing

- **docs:** add illustration assets for README
  ([`67d19fb`](https://github.com/Dicklesworthstone/aadc/commit/67d19fb802c8e200eb992a6856ea37387997b48d))
- **docs:** add performance documentation (`PERF.md`)
  ([`ba8ec85`](https://github.com/Dicklesworthstone/aadc/commit/ba8ec85170328c387f47c93cdfbc6ad7abea3e17))
- **chore:** add MIT License
  ([`d77edc2`](https://github.com/Dicklesworthstone/aadc/commit/d77edc2018d06e530df17054de49e1268e7f70e8))

### 2026-01-20 — Initial Release

- **Initial commit:** aadc CLI with full E2E test suite
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852))
  - Core engine: iterative ASCII/Unicode box-drawing border alignment
  - Detection of corners (`+ \u250c \u2510 \u2514 \u2518 \u2554 \u2557 \u255a \u255d \u256d \u256e \u256f \u2570`), horizontals (`- = ~ \u2500 \u2501 \u2550`),
    verticals (`| \u2502 \u2503 \u2551`), and junctions (`\u252c \u2534 \u251c \u2524 \u253c \u2566 \u2569 \u2560 \u2563 \u256c`)
  - Confidence scoring with configurable `--min-score` threshold
  - Convergence detection with `--max-iters` limit
  - Stdin/stdout pipeline mode and `--in-place` file editing
  - `--verbose` diagnostic output and `--tab-width` expansion
  - `--all` flag to force processing of low-confidence blocks
  - Quick passthrough: files with <1% box-drawing lines skip processing
  - 16 E2E test fixtures (ASCII, Unicode, mixed, edge cases, CJK)
  - 15 basic CLI tests, 17 CLI options tests, 11 unit tests
  - GitHub Actions CI (lint, test, coverage, security audit)
  - GitHub Actions release workflow (cross-platform binary builds)
  - 49 files, 4270 insertions

---

## Capability Summary

For agents and tooling that consume this changelog programmatically, here is a
consolidated view of aadc's current capabilities as of the latest commit:

| Capability | CLI Flag(s) | Added In |
|---|---|---|
| Core border alignment | (default behavior) | `e2077aa` |
| In-place editing | `-i` / `--in-place` | `e2077aa` |
| Verbose diagnostics | `-v` / `--verbose` | `e2077aa` |
| Tab expansion | `-t` / `--tab-width` | `e2077aa` |
| Force all blocks | `-a` / `--all` | `e2077aa` |
| Iteration limit | `-m` / `--max-iters` | `e2077aa` |
| Score threshold | `-s` / `--min-score` | `e2077aa` |
| Multiple file arguments | positional args | `6ce8aab` |
| Confidence presets | `-P` / `--preset` | `d7d9ad8` |
| Unified diff output | `-d` / `--diff` | `d7d9ad8` |
| Dry-run mode | `-n` / `--dry-run` | `d7d9ad8` |
| Backup before edit | `--backup`, `--backup-ext` | `d7d9ad8` |
| JSON output | `--json` | `d7d9ad8` |
| Recursive directory | `-r` / `--recursive` | `2992e32` |
| Glob filtering | `--glob` | `2992e32` |
| Max depth | `--max-depth` | `2992e32` |
| Gitignore awareness | `--no-gitignore` | `2992e32` |
| Config file (`.aadcrc`) | `aadc config` subcommand | `7d50f59` |
| Color control | `--color` | `7d50f59` |
| Line range processing | `-L` / `--lines` | `0094fe2` |
| Statistics summary | (verbose mode) | `078156f` |
| Watch mode | `-w` / `--watch` | `c6f3892` |
| Debounce interval | `--debounce-ms` | `c6f3892` |
| curl\|bash installer | `install.sh` | `cec424c` |
| CJK-aware tab expansion | (automatic) | `72ecb61` |
