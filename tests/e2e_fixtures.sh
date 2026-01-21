#!/usr/bin/env bash
# E2E tests for aadc using fixture files
# Tests all input → expected pairs in tests/fixtures/

set -euo pipefail
shopt -s globstar nullglob

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

if [ -n "${AADC:-}" ]; then
    # shellcheck disable=SC2206
    AADC_CMD=($AADC)
else
    # Build the binary
    echo "Building aadc..."
    cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>/dev/null
    AADC_BIN="${CARGO_TARGET_DIR:-$PROJECT_DIR/target}/release/aadc"

    if [ -x "$AADC_BIN" ]; then
        AADC_CMD=("$AADC_BIN")
    else
        echo "Binary not found at $AADC_BIN, trying cargo run..."
        AADC_CMD=(cargo run --manifest-path "$PROJECT_DIR/Cargo.toml" --quiet --)
    fi
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

log_skip() {
    echo -e "${YELLOW}SKIP${NC}: $1"
}

# Test a single fixture pair
test_fixture() {
    local input_file="$1"
    local expected_file="$2"
    local test_name="$3"

    log_test "$test_name"

    if [ ! -f "$expected_file" ]; then
        log_skip "$test_name (missing expected file)"
        return
    fi

    EXPECTED=$(cat "$expected_file")
    ACTUAL=$("${AADC_CMD[@]}" "$input_file" 2>/dev/null)

    if [ "$ACTUAL" = "$EXPECTED" ]; then
        log_pass "$test_name"
    else
        log_fail "$test_name" "(expected output)" "(different output)"
        # Show diff for debugging
        echo "  --- Diff (first 10 lines) ---"
        diff <(echo "$EXPECTED") <(echo "$ACTUAL") | head -20 || true
        echo "  ---"
    fi
}

# ============================================================================
# FIXTURE PAIRS (auto-discovered)
# ============================================================================
current_category=""
input_files=("$FIXTURES_DIR"/**/*.input.txt)

if [ ${#input_files[@]} -eq 0 ]; then
    log_fail "fixtures discovery" "fixture inputs" "no *.input.txt files found"
else
    for input_file in "${input_files[@]}"; do
        expected_file="${input_file%.input.txt}.expected.txt"
        test_name="${input_file#$FIXTURES_DIR/}"
        test_name="${test_name%.input.txt}"
        category="${test_name%%/*}"
        category_upper="${category^^}"

        if [ "$category_upper" != "$current_category" ]; then
            echo ""
            echo "=== ${category_upper} FIXTURES ==="
            current_category="$category_upper"
        fi

        test_fixture "$input_file" "$expected_file" "$test_name"
    done
fi

# ============================================================================
# DIFF FLAG TESTS
# ============================================================================
echo ""
echo "=== DIFF FLAG TESTS ==="

# Test --diff with changes
test_diff_with_changes() {
    log_test "Diff: outputs unified diff when changes made"

    INPUT="+---+
| a|
+---+"

    ACTUAL=$(echo "$INPUT" | "${AADC_CMD[@]}" --diff 2>/dev/null)

    # Should contain unified diff markers
    if echo "$ACTUAL" | grep -q "^--- a/stdin" && \
       echo "$ACTUAL" | grep -q "^+++ b/stdin" && \
       echo "$ACTUAL" | grep -q "^-| a|" && \
       echo "$ACTUAL" | grep -q "^+| a |"; then
        log_pass "Diff: outputs unified diff when changes made"
    else
        log_fail "Diff: outputs unified diff when changes made" "unified diff format" "$ACTUAL"
    fi
}
test_diff_with_changes

# Test --diff with no changes
test_diff_no_changes() {
    log_test "Diff: no output when no changes"

    INPUT="+---+
| a |
+---+"

    ACTUAL=$(echo "$INPUT" | "${AADC_CMD[@]}" --diff 2>/dev/null)

    if [ -z "$ACTUAL" ]; then
        log_pass "Diff: no output when no changes"
    else
        log_fail "Diff: no output when no changes" "(empty)" "$ACTUAL"
    fi
}
test_diff_no_changes

# Test --diff with verbose
test_diff_verbose() {
    log_test "Diff: works with -v verbose flag"

    INPUT="+---+
| a|
+---+"

    # Capture output (verbose goes to stdout with rich_rust)
    ACTUAL=$(echo "$INPUT" | "${AADC_CMD[@]}" --diff -v 2>/dev/null)

    # Should contain both diff markers AND verbose info
    if echo "$ACTUAL" | grep -q "^--- a/stdin" && \
       echo "$ACTUAL" | grep -q "block"; then
        log_pass "Diff: works with -v verbose flag"
    else
        log_fail "Diff: works with -v verbose flag" "diff + verbose" "$ACTUAL"
    fi
}
test_diff_verbose

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
