# AGENTS.md — aadc (ASCII Art Diagram Corrector)

> Guidelines for AI coding agents working in this Rust codebase.

---

## RULE NUMBER 1: NO FILE DELETION

**YOU ARE NEVER ALLOWED TO DELETE A FILE WITHOUT EXPRESS PERMISSION.** Even a new file that you yourself created, such as a test code file. You have a horrible track record of deleting critically important files or otherwise throwing away tons of expensive work. As a result, you have permanently lost any and all rights to determine that a file or folder should be deleted.

**YOU MUST ALWAYS ASK AND RECEIVE CLEAR, WRITTEN PERMISSION BEFORE EVER DELETING A FILE OR FOLDER OF ANY KIND.**

---

## Irreversible Git & Filesystem Actions — DO NOT EVER BREAK GLASS

1. **Absolutely forbidden commands:** `git reset --hard`, `git clean -fd`, `rm -rf`, or any command that can delete or overwrite code/data must never be run unless the user explicitly provides the exact command and states, in the same message, that they understand and want the irreversible consequences.
2. **No guessing:** If there is any uncertainty about what a command might delete or overwrite, stop immediately and ask the user for specific approval. "I think it's safe" is never acceptable.
3. **Safer alternatives first:** When cleanup or rollbacks are needed, request permission to use non-destructive options (`git status`, `git diff`, `git stash`, copying to backups) before ever considering a destructive command.
4. **Mandatory explicit plan:** Even after explicit user authorization, restate the command verbatim, list exactly what will be affected, and wait for a confirmation that your understanding is correct. Only then may you execute it—if anything remains ambiguous, refuse and escalate.
5. **Document the confirmation:** When running any approved destructive command, record (in the session notes / final response) the exact user text that authorized it, the command actually run, and the execution time. If that record is absent, the operation did not happen.

---

## Git Branch: ONLY Use `main`, NEVER `master`

**The default branch is `main`. The `master` branch exists only for legacy URL compatibility.**

- **All work happens on `main`** — commits, PRs, feature branches all merge to `main`
- **Never reference `master` in code or docs** — if you see `master` anywhere, it's a bug that needs fixing

---

## Toolchain: Rust & Cargo

We only use **Cargo** in this project, NEVER any other package manager.

- **Edition:** Rust 2024 (nightly required — see `rust-toolchain.toml`)
- **Dependency versions:** Explicit versions for stability
- **Configuration:** Cargo.toml only
- **Unsafe code:** Forbidden (`#![forbid(unsafe_code)]`)

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `anyhow` | Error handling with context |
| `clap` | CLI argument parsing with derive macros |
| `rich_rust` | Terminal styling and colored output |

### Release Profile

The release build optimizes for binary size:

```toml
[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit for better optimization
panic = "abort"     # Smaller binary, no unwinding overhead
strip = true        # Remove debug symbols
```

---

## Code Editing Discipline

### No Script-Based Changes

**NEVER** run a script that processes/changes code files in this repo. Brittle regex-based transformations create far more problems than they solve.

- **Always make code changes manually**, even when there are many instances
- For many simple changes: use parallel subagents
- For subtle/complex changes: do them methodically yourself

### No File Proliferation

If you want to change something or add a feature, **revise existing code files in place**.

**NEVER** create variations like:
- `mainV2.rs`
- `main_improved.rs`
- `main_enhanced.rs`

New files are reserved for **genuinely new functionality** that makes zero sense to include in any existing file. The bar for creating new files is **incredibly high**.

---

## Backwards Compatibility

We do not care about backwards compatibility—we're in early development with no users. We want to do things the **RIGHT** way with **NO TECH DEBT**.

- Never create "compatibility shims"
- Never create wrapper functions for deprecated APIs
- Just fix the code directly

---

## Project Overview: aadc

**aadc** (ASCII Art Diagram Corrector) is a CLI tool that fixes misaligned right-hand borders in ASCII diagrams. It uses an iterative correction loop with scoring to achieve clean alignment.

### Core Concepts

1. **Diagram Block Detection**: Identifies "boxy" ASCII diagram blocks heuristically by looking for box-drawing characters (corners, borders, junctions)

2. **Iterative Correction**: Runs a loop that:
   - Analyzes current line states
   - Generates candidate revisions with confidence scores
   - Applies revisions above the threshold
   - Repeats until stable or max iterations reached

3. **Monotone/Insert-Only Edits**: Only adds padding, never removes characters for safety

4. **Box Character Support**: Handles both ASCII (`+`, `-`, `|`) and Unicode box-drawing characters (`┌`, `─`, `│`, etc.)

### Architecture

```
Input → Tab Expansion → Block Detection → Iterative Correction → Output
                              ↓
                        For each block:
                          - Analyze lines
                          - Find target column (rightmost border)
                          - Generate revisions
                          - Score and filter
                          - Apply revisions
                          - Repeat until converged
```

### Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Complete implementation with CLI, core logic, and tests |
| `Cargo.toml` | Dependencies and release optimizations |
| `rust-toolchain.toml` | Nightly toolchain requirement |

### CLI Usage

```bash
# Basic usage - read from stdin, output to stdout
cat diagram.txt | aadc

# Read from file
aadc diagram.txt

# Edit file in place
aadc -i diagram.txt

# Verbose output showing progress
aadc -v diagram.txt

# Adjust parameters
aadc --max-iters 20 --min-score 0.7 diagram.txt

# Process all diagram-like blocks (including low confidence)
aadc --all diagram.txt
```

### CLI Arguments

| Argument | Short | Description | Default |
|----------|-------|-------------|---------|
| `FILE...` | | Input file(s) (stdin if not provided, multiple files supported) | stdin |
| `--in-place` | `-i` | Edit file(s) in place | false |
| `--max-iters` | `-m` | Maximum correction iterations | 10 |
| `--min-score` | `-s` | Minimum score threshold (0.0-1.0) | 0.5 |
| `--preset` | `-P` | Confidence preset: strict (0.8), normal (0.5), aggressive (0.3), relaxed (0.1) | - |
| `--tab-width` | `-t` | Tab expansion width | 4 |
| `--all` | `-a` | Process all diagram-like blocks | false |
| `--verbose` | `-v` | Show correction progress | false |
| `--diff` | `-d` | Show unified diff instead of full output | false |
| `--dry-run` | `-n` | Preview changes (exit 3 if changes would be made) | false |
| `--json` | | Output results as JSON | false |

---

## Compiler Checks (CRITICAL)

**After any substantive code changes, you MUST verify no errors were introduced:**

```bash
# Check for compiler errors and warnings
cargo check --all-targets

# Check for clippy lints
cargo clippy --all-targets -- -D warnings

# Verify formatting
cargo fmt --check
```

If you see errors, **carefully understand and resolve each issue**. Read sufficient context to fix them the RIGHT way.

---

## Testing

### Quick Reference

```bash
# Run all tests (unit + integration)
cargo test

# Run only unit tests
cargo test --lib

# Run integration tests (Rust E2E)
cargo test --test integration

# Run E2E bash test suites
./tests/e2e_basic_cli.sh      # Stdin/stdout, file I/O, exit codes
./tests/e2e_cli_options.sh    # CLI flags and options
./tests/e2e_fixtures.sh       # Fixture-based input/expected tests

# Run comprehensive E2E runner with logging
./tests/e2e_runner.sh         # All suites with detailed log
./tests/e2e_runner.sh -v      # Verbose output
./tests/e2e_runner.sh -f cli  # Filter by pattern
```

### Test Suites

| Suite | Count | Command | Description |
|-------|-------|---------|-------------|
| Unit Tests | 138 | `cargo test --lib` | Core logic, parsing, scoring |
| Integration | 20 | `cargo test --test integration` | Rust E2E tests |
| E2E Basic | 18 | `./tests/e2e_basic_cli.sh` | Stdin, files, exit codes |
| E2E Options | 17 | `./tests/e2e_cli_options.sh` | CLI flags |
| E2E Fixtures | 20 | `./tests/e2e_fixtures.sh` | Input/expected pairs |

### Unit Test Categories

| Test Pattern | Purpose |
|--------------|---------|
| `test_is_corner` | Corner character detection |
| `test_is_horizontal_fill` | Horizontal fill detection |
| `test_is_vertical_border` | Vertical border detection |
| `test_classify_line_*` | Line classification |
| `test_visual_width` | Width calculation |
| `test_expand_tabs` | Tab expansion |
| `test_find_diagram_blocks` | Block detection |
| `test_detect_suffix_border` | Border detection |
| `test_correction_*` | End-to-end correction |
| `test_args_*` | CLI argument parsing |

### E2E Test Fixtures

Test fixtures are in `tests/fixtures/` organized by category:

```
tests/fixtures/
├── ascii/           # ASCII box characters (+, -, |)
├── unicode/         # Unicode box-drawing (┌, ─, │)
├── edge_cases/      # Empty, tabs, whitespace, malformed
├── mixed/           # Multiple diagrams, prose with diagrams
└── large/           # 100+ lines, CJK content
```

Each fixture has `.input.txt` and `.expected.txt` pairs.

### Manual Testing

```bash
# Test with a simple diagram
echo '+----+
| hi |
+----+' | cargo run

# Test verbose mode
echo '+----+
| hi|
+----+' | cargo run -- -v

# Test multiple files (new feature)
cargo run -- tests/fixtures/ascii/*.input.txt
```

### Coverage

CI enforces 80% minimum line coverage via `cargo-llvm-cov`:

```bash
# Generate local coverage report
cargo llvm-cov --html
open target/llvm-cov/html/index.html
```

---

## rich_rust Integration

This project uses `rich_rust` for styled terminal output in verbose mode. The library uses markup syntax for styling:

```rust
// Basic usage
console.print("[bold]Bold text[/]");
console.print("[red]Red text[/]");
console.print("[bold cyan]Bold cyan text[/]");

// Combined styles
console.print("[bold green]Success![/]");
console.print("[dim]Dimmed text[/]");
console.print("[yellow]Warning[/]");
```

### Supported Markup

- Colors: `red`, `green`, `blue`, `cyan`, `yellow`, `magenta`, `white`
- Styles: `bold`, `dim`, `italic`, `underline`
- Close tags: `[/]` closes the current style

---

## Third-Party Library Usage

If you aren't 100% sure how to use a third-party library, **SEARCH ONLINE** to find the latest documentation and best practices.

---

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git add <files>
   git commit -m "..."
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Verify** - All changes committed AND pushed
6. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
