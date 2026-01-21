# Testing Infrastructure Roadmap

> Comprehensive test coverage plan for aadc (ASCII Art Diagram Corrector)

## Current Coverage Analysis

**Status as of 2026-01-21:**
- Line coverage: **74.94%** (447 lines total, 112 missed)
- Function coverage: **91.11%** (45 functions, 4 missed)
- Region coverage: **78.20%**

**Target:** >80% line coverage with no mocks/fakes

---

## Bead Dependency Structure

```
                    ┌─────────────────────────────────────┐
                    │          EPIC: bd-18a               │
                    │   Testing Infrastructure Epic       │
                    └─────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐         ┌───────────────┐         ┌───────────────┐
│ UNIT TESTS    │         │ E2E FIXTURES  │         │ DOCUMENTATION │
│ (parallel)    │         │   bd-p73      │         │   bd-195      │
└───────────────┘         └───────┬───────┘         └───────────────┘
        │                         │                         │
        │                         ▼                         │
        │               ┌───────────────┐                   │
        │               │  E2E TESTS    │                   │
        │               │  (parallel)   │                   │
        │               └───────┬───────┘                   │
        │                       │                           │
        └───────────┬───────────┴───────────────────────────┘
                    │
                    ▼
        ┌───────────────────────────────────────┐
        │           CI WORKFLOW: bd-flx         │
        │  (lint, test, coverage, security)     │
        └───────────────────┬───────────────────┘
                            │
                            ▼
        ┌───────────────────────────────────────┐
        │        RELEASE WORKFLOW: bd-1lr       │
        │    (cross-platform, checksums)        │
        └───────────────────────────────────────┘
```

---

## Phase 1: Unit Tests (No Mocks)

### bd-13s: Character Detection Functions
**Priority:** P1 | **Status:** Ready | **Estimated Tests:** 25+

Test these functions with real character inputs (no mocking):

| Function | Test Cases |
|----------|------------|
| `is_corner()` | All ASCII corners (`+`), all Unicode corners (`┌┐└┘╔╗╚╝╭╮╯╰`), negative cases |
| `is_horizontal_fill()` | All chars (`- ─ ━ ═ ╌ ╍ ┄ ┅ ┈ ┉ ~ =`), negatives |
| `is_vertical_border()` | All chars (`\| │ ┃ ║ ╎ ╏ ┆ ┇ ┊ ┋`), negatives |
| `is_junction()` | All junction types (`┬ ┴ ├ ┤ ┼ ╦ ╩ ╠ ╣ ╬` etc), negatives |
| `is_box_char()` | Composite test covering all box chars |
| `detect_vertical_border()` | Frequency detection with multi-line input |

**Comments:** These are pure functions with no dependencies. Test every documented character plus edge cases (control chars, emoji, CJK).

---

### bd-21x: Line Analysis Functions
**Priority:** P1 | **Status:** Ready | **Estimated Tests:** 30+

| Function | Test Cases |
|----------|------------|
| `classify_line()` | Empty, whitespace, ASCII-only, Unicode, mixed, code snippets, prose |
| `visual_width()` | ASCII (1-width), box chars (1-width), CJK (2-width), emoji (2-width), ZWJ sequences |
| `analyze_line()` | Integration test combining classification + border detection |
| `detect_suffix_border()` | Lines with/without borders, various border chars, edge positions |

**Comments:** Test wide character handling extensively. Unicode width calculation is a common source of bugs.

---

### bd-2ig: Block Detection Functions
**Priority:** P1 | **Status:** Ready | **Estimated Tests:** 20+

| Scenario | Test Cases |
|----------|------------|
| Single block | Simple box, complex box, header/footer |
| Multiple blocks | Two boxes, three boxes, mixed styles |
| Nested structures | Box inside prose, adjacent boxes |
| Gap handling | Single blank line gap, multi-line gap |
| Confidence scoring | High confidence, low confidence, threshold boundaries |
| `--all` flag | Low-confidence blocks included |

**Comments:** Block detection is heuristic-based. Test real-world diagram patterns from documentation.

---

### bd-25l: Revision System
**Priority:** P1 | **Status:** Ready | **Estimated Tests:** 15+

| Component | Test Cases |
|----------|------------|
| `Revision::score()` | PadBeforeSuffixBorder scoring, AddSuffixBorder scoring, edge cases |
| `Revision::apply()` | Padding insertion (small/large), border addition, Unicode preservation |
| Score thresholds | Exactly at threshold, above, below |
| Multi-revision | Multiple revisions on same line |

**Comments:** Verify monotone edits (only adds, never removes). Test that original content is preserved.

---

### bd-3e8: Correction Loop
**Priority:** P1 | **Status:** Ready | **Estimated Tests:** 20+

| Component | Test Cases |
|----------|------------|
| `expand_tabs()` | Tab at position 0, mid-line, tab width 2/4/8 |
| `correct_block()` | Convergence in 1 iteration, multi-iteration, max-iters hit |
| `correct_lines()` | Full integration, empty input, no diagrams |
| Iteration control | --max-iters 1, --max-iters 100 |
| Score filtering | --min-score 0.1, --min-score 0.9 |

**Comments:** Test convergence behavior. Ensure no infinite loops. Verify iteration counts in verbose output.

---

## Phase 2: E2E Integration Tests

### bd-p73: Test Fixtures (PREREQUISITE)
**Priority:** P1 | **Status:** Ready | **Blocks:** All E2E tests

Create input/expected output pairs in `tests/fixtures/`:

```
tests/fixtures/
├── ascii/
│   ├── simple_box.input.txt
│   ├── simple_box.expected.txt
│   ├── nested_boxes.input.txt
│   └── nested_boxes.expected.txt
├── unicode/
│   ├── heavy_borders.input.txt
│   └── heavy_borders.expected.txt
├── mixed/
│   ├── prose_with_diagram.input.txt
│   └── prose_with_diagram.expected.txt
├── edge_cases/
│   ├── empty.input.txt
│   ├── no_diagrams.input.txt
│   ├── already_aligned.input.txt
│   └── single_line.input.txt
└── large/
    ├── 100_lines.input.txt
    └── cjk_content.input.txt
```

---

### bd-155: Basic CLI Functionality
**Priority:** P1 | **Depends on:** bd-p73 | **Estimated Tests:** 10+

| Test | Command | Assertion |
|------|---------|-----------|
| stdin/stdout | `echo "..." \| aadc` | Output matches expected |
| File input | `aadc file.txt` | Output matches expected |
| In-place | `aadc -i file.txt` | File modified correctly |
| Exit codes | Various inputs | 0 on success, non-zero on error |
| Error messages | Invalid file path | Clear error message |

---

### bd-387: CLI Options
**Priority:** P1 | **Depends on:** bd-p73 | **Estimated Tests:** 15+

| Option | Test Cases |
|--------|------------|
| `--max-iters N` | N=1 (may not converge), N=10 (default), N=100 (overkill) |
| `--min-score X` | X=0.1 (aggressive), X=0.5 (default), X=0.9 (conservative) |
| `--tab-width N` | N=2, N=4, N=8 |
| `--all` | Process low-confidence blocks |
| `--verbose` | Output contains progress info |
| Combined | Multiple options together |

---

### bd-mpr: Fixture-Based Tests
**Priority:** P1 | **Depends on:** bd-p73 | **Estimated Tests:** 20+

Test all fixture pairs:
- ASCII diagrams (`+ - |`)
- Unicode diagrams (`┌ ─ │ ╔ ═ ║`)
- Mixed diagrams
- Nested boxes
- Large files (100+ lines)
- CJK content (wide characters)
- Emoji content (if applicable)

---

### bd-1g0: Edge Cases
**Priority:** P1 | **Depends on:** bd-p73 | **Estimated Tests:** 15+

| Edge Case | Expected Behavior |
|-----------|-------------------|
| Empty input | Empty output, exit 0 |
| No diagrams | Passthrough unchanged |
| Already aligned | No modifications |
| Malformed diagrams | Best effort or skip |
| Single-line diagram | Process if confident |
| Tab-heavy content | Expand correctly |
| Binary data | Graceful handling |
| Very long lines | No crash or hang |

---

## Phase 3: GitHub Actions

### bd-flx: CI Workflow
**Priority:** P0 | **Depends on:** All unit + E2E tests | **Status:** Created

Implemented in `.github/workflows/ci.yml`:
- Lint (rustfmt, clippy)
- Test (ubuntu, macos, windows)
- Coverage with 70% threshold
- Security audit (cargo-audit)
- E2E test suite
- Build artifacts (5 targets)

---

### bd-1lr: Release Workflow
**Priority:** P1 | **Depends on:** bd-flx | **Status:** Created

Implemented in `.github/workflows/release.yml`:
- Cross-platform builds (linux-amd64, linux-arm64, darwin-amd64, darwin-arm64, windows-amd64)
- SHA256 checksums for all artifacts
- Automated GitHub Release creation
- Installation instructions in release notes

---

## Phase 4: Documentation

### bd-195: Documentation Updates
**Priority:** P2 | **Depends on:** bd-flx

- [ ] Add coverage badge to README.md
- [ ] Update AGENTS.md with test commands
- [ ] Document fixture format for contributors
- [ ] Add CI badge to README.md

---

## Test Logging Strategy

All tests should produce detailed logs:

```rust
// Unit test logging pattern
#[test]
fn test_is_corner_unicode() {
    let corners = ['┌', '┐', '└', '┘', '╔', '╗', '╚', '╝', '╭', '╮', '╯', '╰'];
    for c in corners {
        println!("Testing corner: {} (U+{:04X})", c, c as u32);
        assert!(is_corner(c), "Expected {} to be detected as corner", c);
    }
}
```

E2E test logging (implemented in CI):
```bash
echo "Test N: description"
<command> && echo "  PASS" || { echo "  FAIL"; exit 1; }
```

---

## Quick Commands

```bash
# Run all unit tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_is_corner

# Run with coverage
cargo llvm-cov --text

# Check coverage threshold
cargo llvm-cov report --summary-only

# List ready beads
br ready --json

# Update bead status
br update bd-XXX --status in_progress
br close bd-XXX --reason "Implemented tests"

# Sync beads
br sync --flush-only
```

---

## Success Criteria

- [ ] Line coverage ≥80%
- [ ] All 11+ existing tests pass
- [ ] 50+ new unit tests (no mocks)
- [ ] 14+ E2E integration tests
- [ ] CI passes on all 3 platforms
- [ ] Release workflow builds all 5 targets
- [ ] All beads closed with completion reasons

---

## Phase 5: Feature Improvements (idea-wizard)

### EPIC: bd-1ih - Feature Improvements Epic
**Priority:** P1 | **Status:** Ready

Contains 15 improvement ideas generated from comprehensive codebase analysis.

```
                    ┌─────────────────────────────────────┐
                    │        EPIC: bd-1ih                 │
                    │   Feature Improvements Epic         │
                    └─────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐         ┌───────────────┐         ┌───────────────┐
│   P2 CORE     │         │   P2 SAFETY   │         │   P3 EXTRAS   │
│ (high value)  │         │  (essential)  │         │  (nice-to-have)│
└───────────────┘         └───────────────┘         └───────────────┘
│ bd-raz: --diff│         │ bd-13d: dry-run│        │ bd-1on: --watch│
│ bd-1c4: --json│         │ bd-3tf: backup │        │ bd-3lo: config │
│ bd-5jn: multi │         │ bd-nci: exit   │        │ bd-107: colors │
│ bd-2am: -r    │         │   codes        │        │ bd-3uw: lines  │
└───────────────┘         └───────────────┘        │ bd-26l: presets│
        │                         │                 │ bd-2tr: stats  │
        │                         │                 │ bd-1zp: optim  │
        │                         │                 │ bd-3i9: hook   │
        └───────────┬─────────────┴─────────────────┘
                    │
            Dependency Flow:
            bd-2am (recursive) → bd-5jn (multi-file)
            bd-3i9 (pre-commit) → bd-raz (diff)
            bd-2tr (stats) → bd-raz (diff)
            bd-1on (watch) → bd-nci (exit codes)
            bd-3lo (config) → bd-26l (presets)
```

---

### P2 Priority Features (High Value)

#### bd-raz: Add --diff flag for unified diff output
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Show unified diff of changes instead of full output. Essential for CI pipelines and code review.

```bash
aadc --diff file.txt  # Shows what would change in diff format
```

---

#### bd-13d: Add --dry-run flag for safe preview
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Show what would change without modifying anything. Critical safety feature for `-i` users.

```bash
aadc --dry-run -i file.txt  # Preview changes before writing
```

---

#### bd-3tf: Add --backup flag for in-place safety
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Create `.bak` file before in-place edit. Simple safety net users expect.

```bash
aadc -i --backup file.txt  # Creates file.txt.bak
```

---

#### bd-1c4: Add --json flag for machine-readable output
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Machine-readable JSON output for tooling integration. Enables editor plugins, CI systems, custom scripts.

```bash
aadc --json file.txt | jq '.blocks'
```

---

#### bd-5jn: Add multiple file support
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Process multiple files in single invocation. Batch processing for documentation repos.

```bash
aadc file1.md file2.md file3.md
aadc *.md
```

---

#### bd-nci: Add semantic exit codes
**Priority:** P2 | **Type:** feature | **Depends on:** bd-1ih

Exit codes that indicate what happened:
- `0`: Success, no changes needed
- `1`: Success, changes were made
- `2`: Error

Essential for CI/CD integration.

---

### P3 Priority Features (Nice-to-Have)

#### bd-1on: Add --watch flag for auto-correction
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih, bd-nci

Auto-correct on file changes using filesystem watcher (`notify` crate).

```bash
aadc --watch file.txt  # Watches and auto-corrects
```

---

#### bd-3lo: Add config file support (.aadcrc)
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih, bd-26l

Store default options per-project in TOML config file.

```toml
# .aadcrc
min_score = 0.3
max_iters = 20
tab_width = 2
```

---

#### bd-107: Add color-coded verbose output
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih

Use ANSI colors in verbose mode. Already have `rich_rust` in dependencies.

---

#### bd-3uw: Add --lines flag for range processing
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih

Process only specific line ranges.

```bash
aadc --lines 10-50 file.txt
```

---

#### bd-2am: Add recursive directory mode
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih, bd-5jn

Process all matching files in directory tree.

```bash
aadc -r --glob '*.md' docs/
```

---

#### bd-26l: Add confidence threshold presets
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih

Named presets instead of numeric `--min-score`.

```bash
aadc --preset strict     # 0.7
aadc --preset normal     # 0.5 (default)
aadc --preset aggressive # 0.3
```

---

#### bd-2tr: Add statistics summary in verbose mode
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih, bd-raz

Show processing statistics at end of verbose output.

```
Summary:
  Files processed: 3
  Blocks found: 7
  Revisions applied: 12
  Time elapsed: 0.042s
```

---

#### bd-1zp: Add stdin passthrough optimization
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih

Quick pre-scan for box chars to speed up pipeline usage. If no diagrams detected, passthrough unchanged.

---

#### bd-3i9: Add git pre-commit hook installer
**Priority:** P3 | **Type:** feature | **Depends on:** bd-1ih, bd-raz

Install pre-commit hook that validates diagrams.

```bash
aadc --install-hook  # Writes to .git/hooks/pre-commit
```

---

## Complete Bead Summary

| ID | Title | Priority | Type | Status |
|----|-------|----------|------|--------|
| **Testing Infrastructure** |
| bd-18a | Testing Infrastructure Epic | P0 | epic | Open |
| bd-flx | GitHub Actions: CI workflow | P0 | task | Open |
| bd-13s | Unit tests: Character detection | P1 | task | Open |
| bd-21x | Unit tests: Line analysis | P1 | task | Open |
| bd-2ig | Unit tests: Block detection | P1 | task | Open |
| bd-25l | Unit tests: Revision system | P1 | task | Open |
| bd-3e8 | Unit tests: Correction loop | P1 | task | Open |
| bd-p73 | E2E test fixtures | P1 | task | Open |
| bd-155 | E2E: Basic CLI | P1 | task | Open |
| bd-387 | E2E: CLI options | P1 | task | Open |
| bd-mpr | E2E: Fixture-based | P1 | task | Open |
| bd-1g0 | E2E: Edge cases | P1 | task | Open |
| bd-1lr | GitHub Actions: Release | P1 | task | Open |
| bd-195 | Documentation updates | P2 | task | Open |
| **Feature Improvements** |
| bd-1ih | Feature Improvements Epic | P1 | epic | Open |
| bd-raz | --diff flag | P2 | feature | Open |
| bd-13d | --dry-run flag | P2 | feature | Open |
| bd-3tf | --backup flag | P2 | feature | Open |
| bd-1c4 | --json flag | P2 | feature | Open |
| bd-5jn | Multiple file support | P2 | feature | Open |
| bd-nci | Semantic exit codes | P2 | feature | Open |
| bd-1on | --watch flag | P3 | feature | Open |
| bd-3lo | Config file support | P3 | feature | Open |
| bd-107 | Color verbose output | P3 | feature | Open |
| bd-3uw | --lines flag | P3 | feature | Open |
| bd-2am | Recursive directory | P3 | feature | Open |
| bd-26l | Confidence presets | P3 | feature | Open |
| bd-2tr | Statistics summary | P3 | feature | Open |
| bd-1zp | Stdin optimization | P3 | feature | Open |
| bd-3i9 | Pre-commit hook | P3 | feature | Open |

**Total:** 30 beads (14 testing + 1 doc + 15 features)
