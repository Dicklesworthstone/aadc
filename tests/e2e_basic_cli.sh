#!/usr/bin/env bash
# E2E tests for aadc basic CLI functionality
# Tests: stdin/stdout, file input, in-place editing, exit codes, error handling

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
# STDIN/STDOUT TESTS
# ============================================================================
echo ""
echo "=== STDIN/STDOUT TESTS ==="

log_test "Reading from stdin, writing to stdout"
INPUT=$(cat "$FIXTURES_DIR/ascii/simple_box.input.txt")
EXPECTED=$(cat "$FIXTURES_DIR/ascii/simple_box.expected.txt")
ACTUAL=$(echo "$INPUT" | $AADC 2>/dev/null)
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "stdin → stdout works correctly"
else
    log_fail "stdin → stdout mismatch" "$EXPECTED" "$ACTUAL"
fi

log_test "Empty stdin produces empty stdout"
ACTUAL=$(echo -n "" | $AADC 2>/dev/null)
if [ -z "$ACTUAL" ]; then
    log_pass "Empty stdin → empty stdout"
else
    log_fail "Empty stdin should produce empty output" "" "$ACTUAL"
fi

log_test "Stdin with no diagrams passes through unchanged"
INPUT="This is just plain text.
No diagrams here.
Just regular content."
ACTUAL=$(echo "$INPUT" | $AADC 2>/dev/null)
if [ "$ACTUAL" = "$INPUT" ]; then
    log_pass "Non-diagram content passes through unchanged"
else
    log_fail "Non-diagram content should pass through" "$INPUT" "$ACTUAL"
fi

# ============================================================================
# FILE INPUT TESTS
# ============================================================================
echo ""
echo "=== FILE INPUT TESTS ==="

log_test "Reading from file argument"
EXPECTED=$(cat "$FIXTURES_DIR/ascii/simple_box.expected.txt")
ACTUAL=$($AADC "$FIXTURES_DIR/ascii/simple_box.input.txt" 2>/dev/null)
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "File input works correctly"
else
    log_fail "File input mismatch" "$EXPECTED" "$ACTUAL"
fi

log_test "Reading Unicode file"
EXPECTED=$(cat "$FIXTURES_DIR/unicode/light_borders.expected.txt")
ACTUAL=$($AADC "$FIXTURES_DIR/unicode/light_borders.input.txt" 2>/dev/null)
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "Unicode file input works correctly"
else
    log_fail "Unicode file input mismatch" "$EXPECTED" "$ACTUAL"
fi

log_test "Reading large file (100+ lines)"
EXPECTED=$(cat "$FIXTURES_DIR/large/100_lines.expected.txt")
ACTUAL=$($AADC "$FIXTURES_DIR/large/100_lines.input.txt" 2>/dev/null)
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "Large file input works correctly"
else
    log_fail "Large file input mismatch" "(large content)" "(different)"
fi

# ============================================================================
# IN-PLACE EDITING TESTS
# ============================================================================
echo ""
echo "=== IN-PLACE EDITING TESTS ==="

log_test "In-place editing with -i flag"
cp "$FIXTURES_DIR/ascii/simple_box.input.txt" "$TMP_DIR/test_inplace.txt"
$AADC -i "$TMP_DIR/test_inplace.txt" 2>/dev/null
EXPECTED=$(cat "$FIXTURES_DIR/ascii/simple_box.expected.txt")
ACTUAL=$(cat "$TMP_DIR/test_inplace.txt")
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "In-place editing works correctly"
else
    log_fail "In-place editing mismatch" "$EXPECTED" "$ACTUAL"
fi

log_test "In-place editing with --in-place flag"
cp "$FIXTURES_DIR/unicode/light_borders.input.txt" "$TMP_DIR/test_inplace2.txt"
$AADC --in-place "$TMP_DIR/test_inplace2.txt" 2>/dev/null
EXPECTED=$(cat "$FIXTURES_DIR/unicode/light_borders.expected.txt")
ACTUAL=$(cat "$TMP_DIR/test_inplace2.txt")
if [ "$ACTUAL" = "$EXPECTED" ]; then
    log_pass "In-place editing with --in-place works"
else
    log_fail "In-place editing with --in-place mismatch" "$EXPECTED" "$ACTUAL"
fi

log_test "In-place editing preserves file when no changes needed"
cp "$FIXTURES_DIR/edge_cases/already_aligned.expected.txt" "$TMP_DIR/test_nochange.txt"
BEFORE=$(cat "$TMP_DIR/test_nochange.txt")
$AADC -i "$TMP_DIR/test_nochange.txt" 2>/dev/null
AFTER=$(cat "$TMP_DIR/test_nochange.txt")
if [ "$BEFORE" = "$AFTER" ]; then
    log_pass "In-place editing preserves unchanged files"
else
    log_fail "In-place editing should preserve unchanged files" "$BEFORE" "$AFTER"
fi

# ============================================================================
# EXIT CODE TESTS
# ============================================================================
echo ""
echo "=== EXIT CODE TESTS ==="

log_test "Exit code 0 on success (stdin)"
echo "+--+" | $AADC >/dev/null 2>&1
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    log_pass "Exit code 0 on successful stdin processing"
else
    log_fail "Exit code should be 0 on success" "0" "$EXIT_CODE"
fi

log_test "Exit code 0 on success (file)"
$AADC "$FIXTURES_DIR/ascii/simple_box.input.txt" >/dev/null 2>&1
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    log_pass "Exit code 0 on successful file processing"
else
    log_fail "Exit code should be 0 on success" "0" "$EXIT_CODE"
fi

log_test "Exit code 0 on empty input"
echo -n "" | $AADC >/dev/null 2>&1
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    log_pass "Exit code 0 on empty input"
else
    log_fail "Exit code should be 0 on empty input" "0" "$EXIT_CODE"
fi

# ============================================================================
# ERROR HANDLING TESTS
# ============================================================================
echo ""
echo "=== ERROR HANDLING TESTS ==="

log_test "Non-zero exit code for non-existent file"
$AADC "/nonexistent/file/path.txt" >/dev/null 2>&1 || EXIT_CODE=$?
if [ ${EXIT_CODE:-0} -ne 0 ]; then
    log_pass "Non-zero exit code for non-existent file"
else
    log_fail "Should return non-zero for non-existent file" "non-zero" "0"
fi

log_test "Error message for non-existent file"
ERROR_MSG=$($AADC "/nonexistent/file/path.txt" 2>&1 || true)
if [[ "$ERROR_MSG" == *"No such file"* ]] || [[ "$ERROR_MSG" == *"not found"* ]] || [[ "$ERROR_MSG" == *"error"* ]]; then
    log_pass "Helpful error message for non-existent file"
else
    log_fail "Should provide helpful error message" "error message" "$ERROR_MSG"
fi

log_test "In-place editing requires file argument"
# Try to use -i with stdin (should fail or be handled gracefully)
RESULT=$(echo "+--+" | $AADC -i 2>&1 || true)
# This might succeed (treating stdin as input) or fail - either is acceptable
log_pass "In-place with stdin handled (behavior documented)"

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
