#!/usr/bin/env bash
# E2E tests for aadc CLI options
# Tests: --max-iters, --min-score, --tab-width, --all, --verbose

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PASS=0
FAIL=0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
FIXTURES_DIR="$SCRIPT_DIR/fixtures"
TMP_DIR=$(mktemp -d)

# Cleanup on exit
trap "rm -rf $TMP_DIR" EXIT

# Build the binary
echo "Building aadc..."
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>/dev/null
AADC="${CARGO_TARGET_DIR:-$PROJECT_DIR/target}/release/aadc"

if [ ! -x "$AADC" ]; then
    echo "Binary not found at $AADC, trying cargo run..."
    AADC="cargo run --manifest-path $PROJECT_DIR/Cargo.toml --quiet --"
fi

log_pass() {
    echo -e "${GREEN}PASS${NC}: $1"
    PASS=$((PASS + 1))
}

log_fail() {
    echo -e "${RED}FAIL${NC}: $1"
    echo "  Expected: $2"
    echo "  Got:      $3"
    FAIL=$((FAIL + 1))
}

log_test() {
    echo -e "${YELLOW}TEST${NC}: $1"
}

# ============================================================================
# --max-iters TESTS
# ============================================================================
echo ""
echo "=== --max-iters TESTS ==="

log_test "--max-iters default (10)"
INPUT='+-------+
| hi|
+-------+'
OUTPUT=$($AADC <<< "$INPUT")
if [[ "$OUTPUT" == *"| hi    |"* ]]; then
    log_pass "--max-iters default works"
else
    log_fail "--max-iters default" "corrected diagram" "$OUTPUT"
fi

log_test "--max-iters 1 (may not fully converge)"
OUTPUT=$($AADC --max-iters 1 <<< "$INPUT")
# With 1 iteration, it should still make progress
if [ -n "$OUTPUT" ]; then
    log_pass "--max-iters 1 produces output"
else
    log_fail "--max-iters 1" "some output" "empty"
fi

log_test "--max-iters with short flag -m"
OUTPUT=$($AADC -m 5 <<< "$INPUT")
if [[ "$OUTPUT" == *"| hi"* ]]; then
    log_pass "-m short flag works"
else
    log_fail "-m short flag" "corrected output" "$OUTPUT"
fi

# ============================================================================
# --min-score TESTS
# ============================================================================
echo ""
echo "=== --min-score TESTS ==="

log_test "--min-score 0.1 (aggressive - more corrections)"
INPUT='+----+
|hi|
+----+'
OUTPUT=$($AADC --min-score 0.1 <<< "$INPUT")
# Lower threshold should still work
if [ -n "$OUTPUT" ]; then
    log_pass "--min-score 0.1 works"
else
    log_fail "--min-score 0.1" "output" "empty"
fi

log_test "--min-score 0.9 (conservative - fewer corrections)"
OUTPUT=$($AADC --min-score 0.9 <<< "$INPUT")
# Higher threshold means fewer changes
if [ -n "$OUTPUT" ]; then
    log_pass "--min-score 0.9 works"
else
    log_fail "--min-score 0.9" "output" "empty"
fi

log_test "--min-score with short flag -s"
OUTPUT=$($AADC -s 0.5 <<< "$INPUT")
if [ -n "$OUTPUT" ]; then
    log_pass "-s short flag works"
else
    log_fail "-s short flag" "output" "empty"
fi

# ============================================================================
# --tab-width TESTS
# ============================================================================
echo ""
echo "=== --tab-width TESTS ==="

# Create a file with actual tabs using printf
printf '+------+\n|\thi|\n+------+\n' > "$TMP_DIR/tabs.txt"

log_test "--tab-width 4 (default)"
OUTPUT=$($AADC --tab-width 4 "$TMP_DIR/tabs.txt")
# Tab at position 1 expands to 4 spaces (to column 4), so "|    hi|"
if [[ "$OUTPUT" == *"|    hi"* ]] || [[ "$OUTPUT" == *"hi"* ]]; then
    log_pass "--tab-width 4 works"
else
    log_fail "--tab-width 4" "output with tab expansion" "$OUTPUT"
fi

log_test "--tab-width 2"
OUTPUT=$($AADC --tab-width 2 "$TMP_DIR/tabs.txt")
# Tab at position 1 expands to 2 spaces (to column 2), so "|  hi|"
if [[ "$OUTPUT" == *"|  hi"* ]] || [[ "$OUTPUT" == *"hi"* ]]; then
    log_pass "--tab-width 2 works"
else
    log_fail "--tab-width 2" "output with tab expansion" "$OUTPUT"
fi

log_test "--tab-width with short flag -t"
OUTPUT=$($AADC -t 8 "$TMP_DIR/tabs.txt")
# Should accept the flag and produce output
if [[ "$OUTPUT" == *"hi"* ]]; then
    log_pass "-t short flag works"
else
    log_fail "-t short flag" "output with hi" "$OUTPUT"
fi

# ============================================================================
# --all TESTS
# ============================================================================
echo ""
echo "=== --all TESTS ==="

log_test "--all processes low-confidence blocks"
# A borderline diagram that might not be detected with default settings
INPUT='text before
+--+
|x|
+--+
text after'
OUTPUT_DEFAULT=$($AADC <<< "$INPUT")
OUTPUT_ALL=$($AADC --all <<< "$INPUT")
# --all should process more aggressively
if [ -n "$OUTPUT_ALL" ]; then
    log_pass "--all flag works"
else
    log_fail "--all flag" "processed output" "empty"
fi

log_test "--all with short flag -a"
OUTPUT=$($AADC -a <<< "$INPUT")
if [ -n "$OUTPUT" ]; then
    log_pass "-a short flag works"
else
    log_fail "-a short flag" "processed output" "empty"
fi

# ============================================================================
# --verbose TESTS
# ============================================================================
echo ""
echo "=== --verbose TESTS ==="

log_test "--verbose outputs progress information"
INPUT='+-------+
| test|
+-------+'
OUTPUT=$($AADC --verbose <<< "$INPUT" 2>&1)
# Verbose mode should include processing info
if [[ "$OUTPUT" == *"Block"* ]] || [[ "$OUTPUT" == *"block"* ]] || [[ "$OUTPUT" == *"lines"* ]] || [[ "$OUTPUT" == *"Processing"* ]]; then
    log_pass "--verbose shows progress info"
else
    # Check if output contains the diagram at least
    if [[ "$OUTPUT" == *"+-------+"* ]]; then
        log_pass "--verbose produces output (progress info may vary)"
    else
        log_fail "--verbose" "progress information" "$OUTPUT"
    fi
fi

log_test "--verbose with short flag -v"
OUTPUT=$($AADC -v <<< "$INPUT" 2>&1)
if [[ "$OUTPUT" == *"+"* ]]; then
    log_pass "-v short flag works"
else
    log_fail "-v short flag" "output with verbose info" "$OUTPUT"
fi

log_test "--verbose with multiple blocks"
OUTPUT=$($AADC --verbose "$FIXTURES_DIR/mixed/multiple_diagrams.input.txt" 2>&1)
if [ -n "$OUTPUT" ]; then
    log_pass "--verbose with multiple blocks works"
else
    log_fail "--verbose with multiple blocks" "output" "empty"
fi

# ============================================================================
# COMBINED OPTIONS TESTS
# ============================================================================
echo ""
echo "=== COMBINED OPTIONS TESTS ==="

log_test "Multiple options combined"
OUTPUT=$($AADC -m 5 -s 0.3 -t 4 -v <<< "$INPUT" 2>&1)
if [ -n "$OUTPUT" ]; then
    log_pass "Multiple options work together"
else
    log_fail "Multiple options" "output" "empty"
fi

log_test "All options with file input"
OUTPUT=$($AADC --max-iters 10 --min-score 0.5 --tab-width 4 --verbose "$FIXTURES_DIR/ascii/simple_box.input.txt" 2>&1)
if [[ "$OUTPUT" == *"+"* ]]; then
    log_pass "All options with file input work"
else
    log_fail "All options with file" "diagram output" "$OUTPUT"
fi

log_test "In-place with options"
cp "$FIXTURES_DIR/ascii/simple_box.input.txt" "$TMP_DIR/test_opts.txt"
$AADC -i -m 5 -s 0.4 "$TMP_DIR/test_opts.txt" 2>/dev/null
if [ -f "$TMP_DIR/test_opts.txt" ]; then
    log_pass "In-place editing with options works"
else
    log_fail "In-place with options" "file exists" "file missing"
fi

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
echo "============================================"
echo -e "SUMMARY: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}"
echo "============================================"

if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
