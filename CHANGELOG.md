# Changelog

All notable changes to **aadc** (ASCII Art Diagram Corrector) are documented here.

Repository: <https://github.com/Dicklesworthstone/aadc>

**v0.1.1 (2026-09-07) is the first tagged release.** The `0.1.0` section below is
the pre-tag history of `main` while `Cargo.toml` read `0.1.0`; it was never tagged
or published. Rather than listing changes in raw diff order, sections are organized
by capability area so readers can quickly find the history of any feature.

Current version in `Cargo.toml`: **0.1.1**

---

## [0.1.1] - 2026-09-07

First tagged release. Two correctness fixes reported against the Markdown and
Unicode handling, one config-validation fix, dependency refreshes, and the
toolchain pin.

### Bug Fixes

- **GFM tables and tagged code fences are no longer treated as diagrams** --
  block detection had no notion of Markdown structure: a table row (`| a | b |`)
  classified as a Strong border line and a shell line such as `--dry-run` as a
  Weak one, so the 3-line lookahead bridged a closing ``` fence into an adjacent
  table and "corrected" both. A new `markdown_protection` pre-pass identifies
  fenced code blocks (backtick or tilde, matching closer, unclosed runs to EOF),
  GFM tables (header + `|:?-+:?|` delimiter row with matching cell counts,
  escaped pipes honoured) and YAML/TOML front matter per the CommonMark/GFM
  rules. Fences whose info string names a language (`bash`, `rust`, `json`,
  `mermaid`, ...) protect their content; untagged fences and diagram-ish tags
  (`text`, `ascii`, `diagram`, ...) keep the normal heuristics. Protected lines
  are hard block boundaries: they never start, extend or bridge a block under
  any preset or `--all`. Fixes [#1](https://github.com/Dicklesworthstone/aadc/issues/1).
  ([`19ec9c6`](https://github.com/Dicklesworthstone/aadc/commit/19ec9c6e94d5464192fec46a9f35784c83ab6034) -- 2026-09-06)
- **Right-border invariant** -- `correct_block` only ever adds a right border to
  a line that already opens with a border glyph (`|`, corner, junction). aadc
  completes a box's missing right side; it never introduces a border into a
  line that has none (`--flag`, prose containing `a | b`, list items).
  ([`19ec9c6`](https://github.com/Dicklesworthstone/aadc/commit/19ec9c6e94d5464192fec46a9f35784c83ab6034) -- 2026-09-06)
- **East-Asian-Ambiguous glyphs measure as 1 column** -- `char_width` was a
  hand-rolled table that counted every non-ASCII, non-box-drawing character
  above U+1100 as 2 columns, so `▶`, `→`, `●`, `█`, `—` and the rest of the
  Ambiguous class were double-width while the (equally Ambiguous) box-drawing
  glyphs next to them were single-width. A perfectly aligned `├───▶│` row
  measured one column too long, became the target width, and every other row
  gained a stray space before its right border. Width now comes from the
  `unicode-width` crate in its non-CJK interpretation (UAX #11 default):
  Wide/Fullwidth are 2, combining marks and default-ignorables are 0,
  everything else including the whole Ambiguous class is 1. `visual_width`
  measures at string level so ZWJ/VS16 sequences count as units.
  Fixes [#2](https://github.com/Dicklesworthstone/aadc/issues/2).
  ([`27e94d2`](https://github.com/Dicklesworthstone/aadc/commit/27e94d20d5b6a963dc3ca34577755581dca7c81b) -- 2026-09-06)
- **Config-file values are validated before merging** -- `validate_args` only
  ran against CLI arguments, so `~/.config/aadc/config.toml` or `.aadc.toml`
  could inject `tab_width = 0` (divide-by-zero panic in `expand_tabs`),
  `tab_width > 16`, `max_iters = 0` (silently disabled every pass) or
  `min_score` outside `0.0..=1.0`. `load_config_file` now applies the same
  bounds checks, with errors anchored to the offending file path.
  ([`4f4761d`](https://github.com/Dicklesworthstone/aadc/commit/4f4761d10cbecc94ac02c1e264ecbdb7ea4cf008) -- 2026-04-24)

### Documentation

- **README: what aadc leaves alone** -- documents the GFM-table, tagged-fence
  and front-matter exclusions and the right-border invariant.
  ([`6e55e65`](https://github.com/Dicklesworthstone/aadc/commit/6e55e6503fa0047e3a94607c1968e063047bf7de) -- 2026-09-06)
- **NEXT_STEPS_PLAN.md** -- reality-check bridge plan toward a first release.
  ([`7c3c782`](https://github.com/Dicklesworthstone/aadc/commit/7c3c7823220181c7b6e04e62840f8bd92ba4ffa2) -- 2026-04-22)
- **AGENTS.md** -- require the OpenAI File Downloader user-agent on curl/web
  fetches.
  ([`4294b2c`](https://github.com/Dicklesworthstone/aadc/commit/4294b2cfe1a81e9e3d6ce0c7dd80c699608d347a) -- 2026-08-21)

### Dependencies

- **unicode-width** -- now a direct dependency (it was already in the graph via
  rich_rust); see the width fix above.
  ([`27e94d2`](https://github.com/Dicklesworthstone/aadc/commit/27e94d20d5b6a963dc3ca34577755581dca7c81b) -- 2026-09-06)
- **notify 6 -> 8.2, similar 2 -> 3, toml 0.8 -> 1.1, criterion 0.5 -> 0.8** --
  four major bumps plus 89 further lockfile entries; no source changes needed.
  ([`2965095`](https://github.com/Dicklesworthstone/aadc/commit/2965095ca1a39baf18202481b985812003bd4869) -- 2026-07-24)
- **Cargo.lock regenerated** from transitive resolution.
  ([`c89057e`](https://github.com/Dicklesworthstone/aadc/commit/c89057e082128bf290757bde5c01e51adae5085c) -- 2026-04-22)

### Build and Toolchain

- **Pinned toolchain** -- `rust-toolchain.toml` pins `nightly-2026-08-31`
  (with `rustfmt` and `clippy`); the clippy gate is run against this exact
  nightly.
  ([`fe5f37c`](https://github.com/Dicklesworthstone/aadc/commit/fe5f37c104f9a134ed8bda1d0460736cbaea3def) -- 2026-09-04)
- **Parallel rustc front-end** -- `.cargo/config.toml` adds `-Z threads=4`.
  ([`e70677e`](https://github.com/Dicklesworthstone/aadc/commit/e70677ebe2ecddadca416f83da894398caeed536) -- 2026-08-11)

### Housekeeping

- **Version 0.1.1** -- `Cargo.toml` / `Cargo.lock` bump for the first tagged
  release.
- **CHANGELOG.md added and rebuilt** from git history with live commit links.
  ([`ae1e1f3`](https://github.com/Dicklesworthstone/aadc/commit/ae1e1f3a2f760d1000f4e355319dd8a7fcd4a32d),
   [`5f32872`](https://github.com/Dicklesworthstone/aadc/commit/5f328729e02374075ff67277246826edde1cc5e9) -- 2026-03-21)
- **gitignore** -- beads runtime metadata, swarm scratch, `.DS_Store`.
  ([`58cc7f7`](https://github.com/Dicklesworthstone/aadc/commit/58cc7f7e68f9900a5c2c69f3ce01ceb4fba07af5),
   [`b2de280`](https://github.com/Dicklesworthstone/aadc/commit/b2de280204ff1c9cab1b941d1c2e0ad47dfef107) -- 2026-04-23 / 2026-04-25)
- **Issue-store syncs and E2E log refresh** during the April reality-check pass.
  ([`f9a4b15`](https://github.com/Dicklesworthstone/aadc/commit/f9a4b15fc5afd88eea7b150c42ff4caffbb56989),
   [`40b28d6`](https://github.com/Dicklesworthstone/aadc/commit/40b28d654c06ba47176d48b268734af65aa47f1b),
   [`8635cbc`](https://github.com/Dicklesworthstone/aadc/commit/8635cbcbfa37a474e7f407281fbc8f64e2a6e3fb),
   [`b2de44f`](https://github.com/Dicklesworthstone/aadc/commit/b2de44f6b49881a82f4a98ca62e0d313a579a3ae),
   [`e7fe3ab`](https://github.com/Dicklesworthstone/aadc/commit/e7fe3abf725cc78643315117e4ab332dc743aa54) -- 2026-04-22)

---

## [0.1.0] -- pre-tag history (never tagged)

### Core Correction Engine

The heart of aadc: iterative right-border alignment for ASCII and Unicode
box-drawing diagrams.

- **Initial implementation** -- heuristic block detection, iterative correction
  loop with convergence detection, confidence scoring with `--min-score`
  threshold, `--max-iters` limit, and monotone (padding-only) edits.
  Recognizes ASCII corners (`+`), horizontals (`- = ~`), verticals (`|`),
  Unicode light/heavy/double borders, rounded corners, and junctions.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Quick passthrough optimization** -- if fewer than 1% of lines contain
  box-drawing characters the input passes through unchanged, making aadc
  essentially free in pipelines over large non-diagram files.
  ([`6ce8aab`](https://github.com/Dicklesworthstone/aadc/commit/6ce8aab02817a36cfa0d91487255267f99ee3d07),
   [`adafd76`](https://github.com/Dicklesworthstone/aadc/commit/adafd7602f28ac7cb1eafe89a292c3e618da6f74) -- 2026-01-21)
- **Enhanced correction algorithms** -- additional refinements to the correction
  logic and scoring heuristics.
  ([`adafd76`](https://github.com/Dicklesworthstone/aadc/commit/adafd7602f28ac7cb1eafe89a292c3e618da6f74) -- 2026-01-21)
- **Architecture refactor** -- restructured `main.rs` for better modularity,
  separating concerns across logical sections.
  ([`28d920d`](https://github.com/Dicklesworthstone/aadc/commit/28d920df299819075ab0645443ad574d6ce74ad5) -- 2026-01-21)

### Input / Output Modes

How content gets into and out of aadc.

- **Stdin / stdout pipeline** -- reads from stdin, writes to stdout when no
  file argument is given. Plays nicely with pipes and shell scripts.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **In-place editing** (`-i` / `--in-place`) -- modify the source file directly.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Multiple file arguments** -- CLI accepts `Vec<PathBuf>`; shell globs work
  (`aadc docs/*.md`). Continue-on-error semantics with summary at the end.
  File headers (`==> file <==`) when outputting to stdout.
  ([`6ce8aab`](https://github.com/Dicklesworthstone/aadc/commit/6ce8aab02817a36cfa0d91487255267f99ee3d07) -- 2026-01-21)
- **Recursive directory mode** (`-r` / `--recursive`) -- walk directory trees
  with `--glob` pattern filtering (default `*.txt,*.md`), `--max-depth` limit,
  and `.gitignore` awareness (`--no-gitignore` to override). Uses the `ignore`
  and `globset` crates.
  ([`2992e32`](https://github.com/Dicklesworthstone/aadc/commit/2992e32106a190f05bace63fd6967d822d71cdd1),
   [`88e1244`](https://github.com/Dicklesworthstone/aadc/commit/88e124418828fc08cd7953c619316b1ecd4fe137) -- 2026-01-21)
- **Unified diff output** (`-d` / `--diff`) -- show a unified diff of changes
  instead of full output. Uses the `similar` crate.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **JSON output** (`--json`) -- machine-readable JSON with `input`,
  `processing`, and `output` fields for programmatic consumption.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **Dry-run mode** (`-n` / `--dry-run`) -- preview changes without writing;
  exits with code 3 if changes would be made.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **Backup before edit** (`--backup`, `--backup-ext`) -- create a `.bak` copy
  before in-place modification.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **Watch mode** (`-w` / `--watch`, `--debounce-ms`) -- continuously monitor a
  file for changes and auto-correct on save. Uses the `notify` crate with
  configurable debounce (default 500 ms). Ctrl+C to stop.
  ([`c6f3892`](https://github.com/Dicklesworthstone/aadc/commit/c6f38924fbcd97ccfd51c063dc0554f1cc83db4f),
   [`654882f`](https://github.com/Dicklesworthstone/aadc/commit/654882fc9e37f41ebb4b8693c70f3b83ec51beca) -- 2026-01-21 / 2026-01-25)

### Tuning and Presets

Controls for how aggressively aadc processes diagrams.

- **Score threshold** (`-s` / `--min-score`) -- configurable confidence
  threshold (0.0--1.0) for applying individual edits.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Iteration limit** (`-m` / `--max-iters`) -- cap on correction passes per
  block.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Force all blocks** (`-a` / `--all`) -- process every detected block,
  including low-confidence ones, and bypass quick passthrough.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Tab expansion** (`-t` / `--tab-width`) -- convert tabs to spaces before
  processing (default 4).
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Confidence presets** (`-P` / `--preset`) -- named threshold profiles:
  Strict (0.8), Normal (0.5), Aggressive (0.3), Relaxed (0.1). Conflicts with
  explicit `--min-score`.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **Line range processing** (`-L` / `--lines`) -- limit correction to specific
  line ranges (`10-50`, `50-`, `-100`, `42`). Supports comma-separated ranges
  with automatic merging.
  ([`0094fe2`](https://github.com/Dicklesworthstone/aadc/commit/0094fe26473929b1236903a46f0de2f07efe4575) -- 2026-01-21)

### Diagnostics and Observability

Verbose output, statistics, and color support.

- **Verbose mode** (`-v` / `--verbose`) -- per-block diagnostic output showing
  iterations, revisions, and convergence status.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Statistics summary** -- verbose mode ends with a summary: blocks
  found/processed/skipped, revisions applied/skipped, elapsed time, and
  throughput (lines/sec). Multi-file mode aggregates via `Stats.merge()`.
  ([`078156f`](https://github.com/Dicklesworthstone/aadc/commit/078156f4f409b9f94e55025daabf0c213af7f996) -- 2026-01-21)
- **Color control** (`--color auto|always|never`) -- terminal color detection
  via `rich_rust`.
  ([`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee) -- 2026-01-21)
- **Semantic exit codes** -- 0 success, 1 general error, 2 invalid arguments,
  3 dry-run-would-change, 4 parse error.
  ([`6ce8aab`](https://github.com/Dicklesworthstone/aadc/commit/6ce8aab02817a36cfa0d91487255267f99ee3d07),
   [`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)

### Configuration

Persistent defaults via config files.

- **Config file system** (`.aadcrc`) -- TOML-based config loaded from local
  directory or `$HOME`. Supports `aadc config init` (create template),
  `aadc config show` (display active config), `aadc config path` (print
  location). Integrates `rich_rust` terminal color detection.
  ([`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee) -- 2026-01-21)

### Installation and Distribution

How users obtain aadc.

- **curl|bash installer** (`install.sh`) -- one-line install with platform
  auto-detection (Linux/macOS, x86_64/aarch64), `--system` for `/usr/local/bin`,
  `--easy-mode` for PATH setup, `--version` for pinned installs, `--from-source`
  for cargo builds.
  ([`cec424c`](https://github.com/Dicklesworthstone/aadc/commit/cec424c36d38f309c959ea7d034ae309e9fe7c1b) -- 2026-01-25)
- **GitHub Actions release workflow** -- cross-platform binary builds for
  Linux and macOS targets, expanded with additional targets alongside the
  installer.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`cec424c`](https://github.com/Dicklesworthstone/aadc/commit/cec424c36d38f309c959ea7d034ae309e9fe7c1b) -- 2026-01-20 / 2026-01-25)
- **Cargo install** -- `cargo install aadc` via crates.io; Edition 2024,
  Rust nightly required.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **Release binary optimization** -- `opt-level = "z"`, LTO, single codegen
  unit, `panic = "abort"`, symbol stripping for minimal binary size.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)

### Unicode and Internationalization

Correct handling of wide and multi-byte characters.

- **Full Unicode box-drawing support** -- light, heavy, double, rounded corners,
  dashed variants, and all junction characters recognized from initial release.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **CJK-aware tab expansion** -- `expand_tabs()` now uses `char_width()` for
  CJK double-width characters instead of assuming width 1. Fixes incorrect tab
  stops when tabs follow CJK content.
  ([`72ecb61`](https://github.com/Dicklesworthstone/aadc/commit/72ecb613c8a3115887fbca48699dbcda0c49d6aa) -- 2026-01-25)

### Bug Fixes

- **Trailing newline preservation** -- `join("\n")` was silently dropping the
  final newline on in-place writes, violating Unix text file conventions. Fixed
  in watch mode, single-file, and recursive write paths. Stdout output was
  already correct.
  ([`d27071b`](https://github.com/Dicklesworthstone/aadc/commit/d27071b696998adc2006e07edf17a44b68974049) -- 2026-01-25)
- **CJK tab expansion** -- see Unicode section above.
  ([`72ecb61`](https://github.com/Dicklesworthstone/aadc/commit/72ecb613c8a3115887fbca48699dbcda0c49d6aa) -- 2026-01-25)
- **Index-out-of-bounds in `Revision::score()`** -- fixed crash when scoring
  edits in multi-block documents.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)

### Testing

- **Unit tests** -- 11 in initial commit, grown to 207+ covering core logic,
  line range parsing, quick scan, tab expansion, and CJK width.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`0094fe2`](https://github.com/Dicklesworthstone/aadc/commit/0094fe26473929b1236903a46f0de2f07efe4575) -- 2026-01-20 / 2026-01-21)
- **Integration tests** (`tests/integration.rs`) -- 25+ Rust integration tests
  covering basic correction, exit codes, error handling, and edge cases.
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)
- **E2E bash suites** -- `e2e_basic_cli.sh` (stdin/stdout, file I/O, exit
  codes), `e2e_cli_options.sh` (CLI flags), `e2e_fixtures.sh` (input/expected
  file pairs including malformed-input edge cases).
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`2dfb677`](https://github.com/Dicklesworthstone/aadc/commit/2dfb6776e7b774287c3b55d2b6022865e01b7bb2) -- 2026-01-20 / 2026-01-21)
- **E2E runner** (`tests/e2e_runner.sh`) -- orchestrator with verbose mode
  (`-v`) and pattern filtering (`-f`).
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)
- **Test fixtures** -- 16 fixture pairs (ASCII, Unicode light/heavy/double/
  rounded, mixed, nested boxes, CJK, 100-line, edge cases: empty, no diagrams,
  single line, tabs, whitespace-only, already aligned, malformed).
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`2dfb677`](https://github.com/Dicklesworthstone/aadc/commit/2dfb6776e7b774287c3b55d2b6022865e01b7bb2) -- 2026-01-20 / 2026-01-21)
- **Benchmarks** -- `benches/correction.rs` (Criterion) and
  `benches/benchmark.sh` (shell-based throughput measurement).
  ([`0066a4f`](https://github.com/Dicklesworthstone/aadc/commit/0066a4f1a443041a44cde469685c953e2bb9c037) -- 2026-01-21)
- **SafeOriginalDir** -- RAII guard for robust cwd save/restore in CI; handles
  macOS GitHub Actions environments where `current_dir()` can fail.
  ([`6177d83`](https://github.com/Dicklesworthstone/aadc/commit/6177d834cbd65f05fd6a2315422bea146e5869ec) -- 2026-01-25)

### CI / CD

- **GitHub Actions CI** -- lint (`clippy -D warnings`), `cargo fmt --check`,
  unit tests, integration tests, coverage (Codecov, 80% threshold), security
  audit.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) -- 2026-01-20)
- **CI streamlining** -- simplified matrix to latest stable Rust, removed
  redundant dependency caching, shallow checkout.
  ([`2dfb677`](https://github.com/Dicklesworthstone/aadc/commit/2dfb6776e7b774287c3b55d2b6022865e01b7bb2) -- 2026-01-21)
- **CI stabilization sprint** (2026-01-23 / 2026-01-24) -- six consecutive
  fixes to get all platforms green:
  - Resolve clippy warnings
    ([`8aac65f`](https://github.com/Dicklesworthstone/aadc/commit/8aac65fd2f824e365136351b3a64c10ced500dd0))
  - Stabilize CWD tests and bench binary lookup
    ([`6dd17e6`](https://github.com/Dicklesworthstone/aadc/commit/6dd17e6e47df88e482d252e72b8cdc404993d975))
  - Use git dependency for `rich_rust` (pre-crates.io publication)
    ([`b710f72`](https://github.com/Dicklesworthstone/aadc/commit/b710f7262f03336564f866185fa32e2b16df78c2))
  - Use cargo-provided binary path in integration tests
    ([`9b41ad5`](https://github.com/Dicklesworthstone/aadc/commit/9b41ad52f930a021391393592a4529e417afb74a))
  - Build binary before running tests
    ([`636c0d9`](https://github.com/Dicklesworthstone/aadc/commit/636c0d963e2a45a1bcaa94995a528ccf953f9801))
  - Run tests single-threaded to avoid mutex poisoning
    ([`bf19fe2`](https://github.com/Dicklesworthstone/aadc/commit/bf19fe296b9dc101ecc2830d44587f8015dae784))

### Input Validation and Error Handling

- **Tab-width validation** -- must be between 1 and 16.
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)
- **100 MB file size limit** -- prevents memory issues on oversized inputs.
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)
- **High max-iters warning** -- diagnostic when `--max-iters` exceeds 100.
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)

### Documentation

- **README** -- comprehensive usage guide with quick-start, architecture
  diagram, command reference, troubleshooting, FAQ, comparison table.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-20 / 2026-01-21)
- **Code documentation** -- `#![warn(missing_docs)]` lint, full doc coverage
  for `LineKind`, `AnalyzedLine`, `SuffixBorder`, `DiagramBlock`, `Revision`,
  `visual_width`, `correct_block`.
  ([`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) -- 2026-01-21)
- **Performance documentation** (`PERF.md`) -- benchmark results and
  performance characteristics.
  ([`ba8ec85`](https://github.com/Dicklesworthstone/aadc/commit/ba8ec85170328c387f47c93cdfbc6ad7abea3e17) -- 2026-01-21)
- **Illustration assets** -- `aadc_illustration.webp` for README header.
  ([`67d19fb`](https://github.com/Dicklesworthstone/aadc/commit/67d19fb802c8e200eb992a6856ea37387997b48d) -- 2026-01-21)
- **GitHub social preview** -- 1280x640 OG share image.
  ([`474f4b4`](https://github.com/Dicklesworthstone/aadc/commit/474f4b4b76e13ed58311cd9a892408489c9d2449) -- 2026-02-21)
- **AGENTS.md** -- multi-agent development conventions, cass tool reference.
  ([`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852),
   [`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8),
   [`65ffbf2`](https://github.com/Dicklesworthstone/aadc/commit/65ffbf2cd0367d87ac085790bc32c909afd16d45),
   [`3bfd864`](https://github.com/Dicklesworthstone/aadc/commit/3bfd86426d591367c2d8450b8644516804bd901b) -- 2026-01-20 .. 2026-02-25)

### Dependencies

- **rich_rust** -- terminal color detection and formatting; migrated from
  pre-release git ref to crates.io v0.2.0 for reproducible builds.
  ([`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee),
   [`74382fd`](https://github.com/Dicklesworthstone/aadc/commit/74382fd1c75720c4c16a66869dccec7679a481be) -- 2026-01-21 / 2026-02-15)
- **notify** + **ctrlc** -- file system watching for `--watch` mode.
  ([`c6f3892`](https://github.com/Dicklesworthstone/aadc/commit/c6f38924fbcd97ccfd51c063dc0554f1cc83db4f) -- 2026-01-21)
- **globset** + **ignore** -- recursive directory walking with gitignore support.
  ([`88e1244`](https://github.com/Dicklesworthstone/aadc/commit/88e124418828fc08cd7953c619316b1ecd4fe137) -- 2026-01-21)
- **similar** -- unified diff generation.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **serde** + **serde_json** -- JSON output serialization.
  ([`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) -- 2026-01-21)
- **dirs** + **toml** -- config file discovery and parsing.
  ([`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee) -- 2026-01-21)
- **criterion** -- benchmark harness (dev-dependency).
  ([`0066a4f`](https://github.com/Dicklesworthstone/aadc/commit/0066a4f1a443041a44cde469685c953e2bb9c037) -- 2026-01-21)

### Licensing

- **MIT License** added on 2026-01-21.
  ([`d77edc2`](https://github.com/Dicklesworthstone/aadc/commit/d77edc2018d06e530df17054de49e1268e7f70e8))
- **MIT + OpenAI/Anthropic Rider** -- updated license to restrict use by
  OpenAI, Anthropic, and affiliates without express written permission.
  ([`57df125`](https://github.com/Dicklesworthstone/aadc/commit/57df125085bbf3d4ea5b9b7d64768e55b2318dce),
   [`bc5b8c4`](https://github.com/Dicklesworthstone/aadc/commit/bc5b8c46e4fef629dfb420c48ef950849af6ff05) -- 2026-02-21 / 2026-02-22)

### Housekeeping

- Remove stale macOS resource fork file.
  ([`57f6537`](https://github.com/Dicklesworthstone/aadc/commit/57f65375ad67212dd59c79b05dda949c6aa13e4d) -- 2026-03-13)
- Apply `cargo fmt`, remove stale `dead_code` annotations.
  ([`6c4f1e9`](https://github.com/Dicklesworthstone/aadc/commit/6c4f1e951d4fdde3dde387428fd37dcc83391a1b),
   [`1bde9bf`](https://github.com/Dicklesworthstone/aadc/commit/1bde9bfa4daacc317ec29f05fc5a1194cd0f8f0d) -- 2026-01-23 / 2026-01-25)
- `.gitignore` updates: `/target/`, ephemeral agent files, `a.out`.
  ([`03dc4a9`](https://github.com/Dicklesworthstone/aadc/commit/03dc4a9463d7f86e5eb514925994f3f209a66f76),
   [`0f65064`](https://github.com/Dicklesworthstone/aadc/commit/0f65064b43c8bf49c923ffdefb618871943d6cbc),
   [`2f42a09`](https://github.com/Dicklesworthstone/aadc/commit/2f42a09232b8ff52d8deec0cf14ca74d5263d286) -- 2026-01-24 / 2026-01-25)
- Update E2E test results logs.
  ([`4fd16e3`](https://github.com/Dicklesworthstone/aadc/commit/4fd16e3db47132dc41b16d2ed9ad709ee41cb364),
   [`bd4255b`](https://github.com/Dicklesworthstone/aadc/commit/bd4255b2e8d1f71a4224132a431199f826f05213) -- 2026-02-01 / 2026-02-02)
- Parallel agent work -- beads and README sync.
  ([`654882f`](https://github.com/Dicklesworthstone/aadc/commit/654882fc9e37f41ebb4b8693c70f3b83ec51beca),
   [`bc0740d`](https://github.com/Dicklesworthstone/aadc/commit/bc0740d87a05e0f97a0b7f81f6ab30bda2db7396) -- 2026-01-25)

---

## Commit Index

All 69 commits on `main` preceding the v0.1.1 release commit, oldest first.

| Date | Hash | Summary |
|------|------|---------|
| 2026-01-20 | [`e2077aa`](https://github.com/Dicklesworthstone/aadc/commit/e2077aa08752dd3041bab2893923bea845c5c852) | Initial commit: aadc CLI with E2E test suite |
| 2026-01-21 | [`6ce8aab`](https://github.com/Dicklesworthstone/aadc/commit/6ce8aab02817a36cfa0d91487255267f99ee3d07) | Add multiple file support |
| 2026-01-21 | [`d7d9ad8`](https://github.com/Dicklesworthstone/aadc/commit/d7d9ad88e228a443fe131c30c290aacea36a97af) | Add confidence presets and JSON output support |
| 2026-01-21 | [`2dfb677`](https://github.com/Dicklesworthstone/aadc/commit/2dfb6776e7b774287c3b55d2b6022865e01b7bb2) | Streamline CI and add fixture tests |
| 2026-01-21 | [`67d19fb`](https://github.com/Dicklesworthstone/aadc/commit/67d19fb802c8e200eb992a6856ea37387997b48d) | Add illustration assets for README |
| 2026-01-21 | [`544288d`](https://github.com/Dicklesworthstone/aadc/commit/544288dd14753e87857a9d7dffc81314ec3b99f8) | Implement error handling, E2E tests, and documentation |
| 2026-01-21 | [`0066a4f`](https://github.com/Dicklesworthstone/aadc/commit/0066a4f1a443041a44cde469685c953e2bb9c037) | Add benchmarks and update dependencies |
| 2026-01-21 | [`adafd76`](https://github.com/Dicklesworthstone/aadc/commit/adafd7602f28ac7cb1eafe89a292c3e618da6f74) | Enhance correction algorithms |
| 2026-01-21 | [`bfe89cd`](https://github.com/Dicklesworthstone/aadc/commit/bfe89cdc8573389ab6bf89fcfb928940be252984) | Close bd-1zp |
| 2026-01-21 | [`ba8ec85`](https://github.com/Dicklesworthstone/aadc/commit/ba8ec85170328c387f47c93cdfbc6ad7abea3e17) | Add performance documentation |
| 2026-01-21 | [`5923621`](https://github.com/Dicklesworthstone/aadc/commit/5923621b9d611bf39b87aedb3e53316dd932a014) | Update beads |
| 2026-01-21 | [`f8fc448`](https://github.com/Dicklesworthstone/aadc/commit/f8fc44894dfa225249c9e3f50d9b86bfd9fee8fc) | Track progress in beads |
| 2026-01-21 | [`88e1244`](https://github.com/Dicklesworthstone/aadc/commit/88e124418828fc08cd7953c619316b1ecd4fe137) | Update Cargo config and main implementation |
| 2026-01-21 | [`2992e32`](https://github.com/Dicklesworthstone/aadc/commit/2992e32106a190f05bace63fd6967d822d71cdd1) | Add recursive directory mode |
| 2026-01-21 | [`7d50f59`](https://github.com/Dicklesworthstone/aadc/commit/7d50f597dcb1e380122514167040288627360fee) | Add config file system, color mode, config subcommand |
| 2026-01-21 | [`28d920d`](https://github.com/Dicklesworthstone/aadc/commit/28d920df299819075ab0645443ad574d6ce74ad5) | Refactor main with improved architecture |
| 2026-01-21 | [`52b0825`](https://github.com/Dicklesworthstone/aadc/commit/52b0825b46437ab87dd8a030e8ff46277a6e696c) | Update implementation and tests |
| 2026-01-21 | [`16ad5c0`](https://github.com/Dicklesworthstone/aadc/commit/16ad5c0477929d289865b58bf00c6dbe16dff89d) | Update implementation |
| 2026-01-21 | [`9e91df2`](https://github.com/Dicklesworthstone/aadc/commit/9e91df21c3933599e0bd4c4c1a52d6b0e7f1f666) | Update implementation |
| 2026-01-21 | [`0094fe2`](https://github.com/Dicklesworthstone/aadc/commit/0094fe26473929b1236903a46f0de2f07efe4575) | Add --lines flag for range processing |
| 2026-01-21 | [`078156f`](https://github.com/Dicklesworthstone/aadc/commit/078156f4f409b9f94e55025daabf0c213af7f996) | Add statistics summary in verbose mode |
| 2026-01-21 | [`c6f3892`](https://github.com/Dicklesworthstone/aadc/commit/c6f38924fbcd97ccfd51c063dc0554f1cc83db4f) | Add watch mode and enhance CLI |
| 2026-01-21 | [`d77edc2`](https://github.com/Dicklesworthstone/aadc/commit/d77edc2018d06e530df17054de49e1268e7f70e8) | Add MIT License |
| 2026-01-23 | [`1bde9bf`](https://github.com/Dicklesworthstone/aadc/commit/1bde9bfa4daacc317ec29f05fc5a1194cd0f8f0d) | Fix cargo fmt formatting |
| 2026-01-24 | [`8aac65f`](https://github.com/Dicklesworthstone/aadc/commit/8aac65fd2f824e365136351b3a64c10ced500dd0) | Resolve clippy warnings in CI |
| 2026-01-24 | [`6dd17e6`](https://github.com/Dicklesworthstone/aadc/commit/6dd17e6e47df88e482d252e72b8cdc404993d975) | Stabilize CWD tests and bench binary lookup |
| 2026-01-24 | [`b710f72`](https://github.com/Dicklesworthstone/aadc/commit/b710f7262f03336564f866185fa32e2b16df78c2) | Use git dependency for rich_rust |
| 2026-01-24 | [`9b41ad5`](https://github.com/Dicklesworthstone/aadc/commit/9b41ad52f930a021391393592a4529e417afb74a) | Use cargo-provided binary path in integration tests |
| 2026-01-24 | [`636c0d9`](https://github.com/Dicklesworthstone/aadc/commit/636c0d963e2a45a1bcaa94995a528ccf953f9801) | Build binary before running tests |
| 2026-01-24 | [`bf19fe2`](https://github.com/Dicklesworthstone/aadc/commit/bf19fe296b9dc101ecc2830d44587f8015dae784) | Run tests single-threaded to avoid mutex poisoning |
| 2026-01-24 | [`0f65064`](https://github.com/Dicklesworthstone/aadc/commit/0f65064b43c8bf49c923ffdefb618871943d6cbc) | Add ephemeral file patterns to gitignore |
| 2026-01-24 | [`2f42a09`](https://github.com/Dicklesworthstone/aadc/commit/2f42a09232b8ff52d8deec0cf14ca74d5263d286) | Add a.out to gitignore |
| 2026-01-25 | [`cec424c`](https://github.com/Dicklesworthstone/aadc/commit/cec424c36d38f309c959ea7d034ae309e9fe7c1b) | Add install.sh and improve release workflow |
| 2026-01-25 | [`654882f`](https://github.com/Dicklesworthstone/aadc/commit/654882fc9e37f41ebb4b8693c70f3b83ec51beca) | Parallel agent work (README) |
| 2026-01-25 | [`bc0740d`](https://github.com/Dicklesworthstone/aadc/commit/bc0740d87a05e0f97a0b7f81f6ab30bda2db7396) | Parallel agent work (beads) |
| 2026-01-25 | [`6c4f1e9`](https://github.com/Dicklesworthstone/aadc/commit/6c4f1e951d4fdde3dde387428fd37dcc83391a1b) | Apply cargo fmt, remove dead_code comment |
| 2026-01-25 | [`72ecb61`](https://github.com/Dicklesworthstone/aadc/commit/72ecb613c8a3115887fbca48699dbcda0c49d6aa) | Fix CJK character width in tab expansion |
| 2026-01-25 | [`d27071b`](https://github.com/Dicklesworthstone/aadc/commit/d27071b696998adc2006e07edf17a44b68974049) | Preserve trailing newline when writing files |
| 2026-01-25 | [`03dc4a9`](https://github.com/Dicklesworthstone/aadc/commit/03dc4a9463d7f86e5eb514925994f3f209a66f76) | Add /target/ to gitignore |
| 2026-01-25 | [`6177d83`](https://github.com/Dicklesworthstone/aadc/commit/6177d834cbd65f05fd6a2315422bea146e5869ec) | Add SafeOriginalDir for robust cwd handling in CI |
| 2026-02-01 | [`4fd16e3`](https://github.com/Dicklesworthstone/aadc/commit/4fd16e3db47132dc41b16d2ed9ad709ee41cb364) | Update E2E test results log |
| 2026-02-02 | [`bd4255b`](https://github.com/Dicklesworthstone/aadc/commit/bd4255b2e8d1f71a4224132a431199f826f05213) | Update E2E test results with latest run |
| 2026-02-14 | [`65ffbf2`](https://github.com/Dicklesworthstone/aadc/commit/65ffbf2cd0367d87ac085790bc32c909afd16d45) | Update AGENTS.md multi-agent conventions |
| 2026-02-15 | [`74382fd`](https://github.com/Dicklesworthstone/aadc/commit/74382fd1c75720c4c16a66869dccec7679a481be) | Upgrade rich_rust to crates.io v0.2.0 |
| 2026-02-21 | [`474f4b4`](https://github.com/Dicklesworthstone/aadc/commit/474f4b4b76e13ed58311cd9a892408489c9d2449) | Add GitHub social preview image |
| 2026-02-21 | [`57df125`](https://github.com/Dicklesworthstone/aadc/commit/57df125085bbf3d4ea5b9b7d64768e55b2318dce) | Update license to MIT + OpenAI/Anthropic Rider |
| 2026-02-22 | [`bc5b8c4`](https://github.com/Dicklesworthstone/aadc/commit/bc5b8c46e4fef629dfb420c48ef950849af6ff05) | Update README license references |
| 2026-02-25 | [`3bfd864`](https://github.com/Dicklesworthstone/aadc/commit/3bfd86426d591367c2d8450b8644516804bd901b) | Add cass tool reference to AGENTS.md |
| 2026-03-13 | [`57f6537`](https://github.com/Dicklesworthstone/aadc/commit/57f65375ad67212dd59c79b05dda949c6aa13e4d) | Remove stale macOS resource fork file |
| 2026-03-21 | [`ae1e1f3`](https://github.com/Dicklesworthstone/aadc/commit/ae1e1f3a2f760d1000f4e355319dd8a7fcd4a32d) | Add comprehensive CHANGELOG.md documenting project history |
| 2026-03-21 | [`5f32872`](https://github.com/Dicklesworthstone/aadc/commit/5f328729e02374075ff67277246826edde1cc5e9) | Rebuild CHANGELOG.md from git history with live commit links |
| 2026-04-22 | [`c89057e`](https://github.com/Dicklesworthstone/aadc/commit/c89057e082128bf290757bde5c01e51adae5085c) | Regenerate Cargo.lock from transitive resolution |
| 2026-04-22 | [`f9a4b15`](https://github.com/Dicklesworthstone/aadc/commit/f9a4b15fc5afd88eea7b150c42ff4caffbb56989) | Sync issue store state (reality-check pass) |
| 2026-04-22 | [`40b28d6`](https://github.com/Dicklesworthstone/aadc/commit/40b28d654c06ba47176d48b268734af65aa47f1b) | Refresh tests/e2e_results.log |
| 2026-04-22 | [`7c3c782`](https://github.com/Dicklesworthstone/aadc/commit/7c3c7823220181c7b6e04e62840f8bd92ba4ffa2) | Add NEXT_STEPS_PLAN.md |
| 2026-04-22 | [`8635cbc`](https://github.com/Dicklesworthstone/aadc/commit/8635cbcbfa37a474e7f407281fbc8f64e2a6e3fb) | Add bd-b1f (stability policy for --json output) |
| 2026-04-22 | [`b2de44f`](https://github.com/Dicklesworthstone/aadc/commit/b2de44f6b49881a82f4a98ca62e0d313a579a3ae) | File new backlog items from NEXT_STEPS_PLAN follow-up |
| 2026-04-22 | [`e7fe3ab`](https://github.com/Dicklesworthstone/aadc/commit/e7fe3abf725cc78643315117e4ab332dc743aa54) | Sync issue store (23 issues touched) |
| 2026-04-23 | [`58cc7f7`](https://github.com/Dicklesworthstone/aadc/commit/58cc7f7e68f9900a5c2c69f3ce01ceb4fba07af5) | Gitignore beads runtime metadata + swarm coordination scratch |
| 2026-04-24 | [`4f4761d`](https://github.com/Dicklesworthstone/aadc/commit/4f4761d10cbecc94ac02c1e264ecbdb7ea4cf008) | Validate file-config values before merging into runtime Config |
| 2026-04-25 | [`b2de280`](https://github.com/Dicklesworthstone/aadc/commit/b2de280204ff1c9cab1b941d1c2e0ad47dfef107) | Gitignore .DS_Store |
| 2026-07-24 | [`2965095`](https://github.com/Dicklesworthstone/aadc/commit/2965095ca1a39baf18202481b985812003bd4869) | Bump notify 6 -> 8.2, similar 2 -> 3, toml 0.8 -> 1.1, criterion 0.5 -> 0.8 |
| 2026-08-11 | [`e70677e`](https://github.com/Dicklesworthstone/aadc/commit/e70677ebe2ecddadca416f83da894398caeed536) | Enable parallel rustc front-end (-Z threads=4) |
| 2026-08-21 | [`4294b2c`](https://github.com/Dicklesworthstone/aadc/commit/4294b2cfe1a81e9e3d6ce0c7dd80c699608d347a) | Require OpenAI File Downloader user-agent on curl/web fetches (AGENTS.md) |
| 2026-09-04 | [`fe5f37c`](https://github.com/Dicklesworthstone/aadc/commit/fe5f37c104f9a134ed8bda1d0460736cbaea3def) | Pin nightly-2026-08-31 |
| 2026-09-06 | [`19ec9c6`](https://github.com/Dicklesworthstone/aadc/commit/19ec9c6e94d5464192fec46a9f35784c83ab6034) | Stop treating GFM tables and tagged code fences as diagrams (#1) |
| 2026-09-06 | [`27e94d2`](https://github.com/Dicklesworthstone/aadc/commit/27e94d20d5b6a963dc3ca34577755581dca7c81b) | Measure East-Asian-Ambiguous glyphs as 1 column via unicode-width (#2) |
| 2026-09-06 | [`6e55e65`](https://github.com/Dicklesworthstone/aadc/commit/6e55e6503fa0047e3a94607c1968e063047bf7de) | Describe the Markdown constructs aadc leaves alone (README) |
