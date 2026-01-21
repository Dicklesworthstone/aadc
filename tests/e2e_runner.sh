#!/usr/bin/env bash
# Comprehensive E2E Test Runner for aadc with detailed logging
# Usage: ./tests/e2e_runner.sh [--verbose] [--filter PATTERN]
#
# This script orchestrates all E2E test suites and produces a detailed log file

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
LOG_FILE="${SCRIPT_DIR}/e2e_results.log"
TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

VERBOSE=false
FILTER=""
TOTAL_PASSED=0
TOTAL_FAILED=0
TOTAL_SKIPPED=0

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --filter|-f)
            FILTER="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -v, --verbose     Show detailed output for all tests"
            echo "  -f, --filter PAT  Only run test suites matching PAT"
            echo "  -h, --help        Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Initialize log file
initialize_log() {
    cat > "$LOG_FILE" << EOF
================================================================================
AADC E2E Test Results
================================================================================
Run Time:    $TIMESTAMP
Host:        $(hostname)
Directory:   $PROJECT_ROOT
Filter:      ${FILTER:-"(none)"}
Verbose:     $VERBOSE
================================================================================

EOF
}

log() {
    local level="$1"
    shift
    local msg="$*"
    local ts=$(date '+%H:%M:%S')

    # Always write to log file
    echo "[$ts] [$level] $msg" >> "$LOG_FILE"

    # Console output based on level and verbosity
    if [[ "$VERBOSE" == "true" ]] || [[ "$level" == "ERROR" ]] || [[ "$level" == "RESULT" ]] || [[ "$level" == "SUITE" ]]; then
        case "$level" in
            ERROR)   echo -e "${RED}[$level]${NC} $msg" ;;
            PASS)    echo -e "${GREEN}[$level]${NC} $msg" ;;
            FAIL)    echo -e "${RED}[$level]${NC} $msg" ;;
            SKIP)    echo -e "${YELLOW}[$level]${NC} $msg" ;;
            SUITE)   echo -e "${BOLD}${BLUE}[$level]${NC} $msg" ;;
            RESULT)  echo -e "${BOLD}$msg${NC}" ;;
            INFO)    echo -e "${BLUE}[$level]${NC} $msg" ;;
            *)       echo "[$level] $msg" ;;
        esac
    fi
}

# Run a test suite and capture results
run_suite() {
    local suite_name="$1"
    local suite_script="$2"

    # Apply filter if specified
    if [[ -n "$FILTER" ]] && [[ ! "$suite_name" =~ $FILTER ]]; then
        log "SKIP" "Suite: $suite_name (filtered out)"
        return 0
    fi

    log "SUITE" "Running: $suite_name"
    echo "" >> "$LOG_FILE"
    echo "--- Suite: $suite_name ---" >> "$LOG_FILE"

    local start_time=$(date +%s)
    local output
    local exit_code=0

    # Capture output and exit code
    output=$("$suite_script" 2>&1) || exit_code=$?

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    # Log full output
    echo "$output" >> "$LOG_FILE"
    echo "" >> "$LOG_FILE"

    # Parse results from output
    local passed=$(echo "$output" | grep -oP '\d+(?= passed)' | tail -1 || echo "0")
    local failed=$(echo "$output" | grep -oP '\d+(?= failed)' | tail -1 || echo "0")

    # Update totals
    TOTAL_PASSED=$((TOTAL_PASSED + passed))
    TOTAL_FAILED=$((TOTAL_FAILED + failed))

    # Log summary
    if [[ $exit_code -eq 0 ]]; then
        log "PASS" "Suite $suite_name: $passed passed, $failed failed (${duration}s)"
    else
        log "FAIL" "Suite $suite_name: $passed passed, $failed failed (${duration}s) [exit code: $exit_code]"
    fi

    return $exit_code
}

# Build the project first
build_project() {
    log "INFO" "Building aadc in release mode..."
    echo "" >> "$LOG_FILE"
    echo "--- Build Output ---" >> "$LOG_FILE"

    if cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml" >> "$LOG_FILE" 2>&1; then
        log "PASS" "Build successful"
        return 0
    else
        log "ERROR" "Build failed - see log file for details"
        return 1
    fi
}

# Run cargo tests
run_cargo_tests() {
    log "SUITE" "Running: cargo unit tests"
    echo "" >> "$LOG_FILE"
    echo "--- Cargo Tests ---" >> "$LOG_FILE"

    local start_time=$(date +%s)
    local output
    local exit_code=0

    output=$(cargo test --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>&1) || exit_code=$?

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    echo "$output" >> "$LOG_FILE"
    echo "" >> "$LOG_FILE"

    # Parse test results - look for the test result line
    local test_line=$(echo "$output" | grep -E "^test result:" | tail -1 || echo "")
    local passed=0
    local failed=0

    if [[ -n "$test_line" ]]; then
        passed=$(echo "$test_line" | grep -oP '\d+(?= passed)' || echo "0")
        failed=$(echo "$test_line" | grep -oP '\d+(?= failed)' || echo "0")
    fi

    # Ensure passed/failed are numbers
    passed=${passed:-0}
    failed=${failed:-0}

    if [[ $exit_code -eq 0 ]]; then
        log "PASS" "Cargo tests: $passed passed (${duration}s)"
        TOTAL_PASSED=$((TOTAL_PASSED + passed))
    else
        log "FAIL" "Cargo tests: $passed passed, $failed failed (${duration}s)"
        TOTAL_PASSED=$((TOTAL_PASSED + passed))
        TOTAL_FAILED=$((TOTAL_FAILED + failed))
    fi

    return $exit_code
}

# Main execution
main() {
    local overall_exit=0

    initialize_log

    echo -e "${BOLD}AADC E2E Test Runner${NC}"
    echo "===================="
    echo ""

    # Build first
    if ! build_project; then
        log "ERROR" "Build failed, aborting tests"
        exit 1
    fi

    echo ""
    log "INFO" "Starting test suites..."
    echo ""

    # Run cargo unit tests (if not filtered out)
    if [[ -z "$FILTER" ]] || [[ "cargo" =~ $FILTER ]]; then
        run_cargo_tests || overall_exit=1
    fi

    # Run E2E test suites
    local suites=(
        "e2e_basic_cli:$SCRIPT_DIR/e2e_basic_cli.sh"
        "e2e_cli_options:$SCRIPT_DIR/e2e_cli_options.sh"
        "e2e_fixtures:$SCRIPT_DIR/e2e_fixtures.sh"
    )

    for suite_entry in "${suites[@]}"; do
        local suite_name="${suite_entry%%:*}"
        local suite_script="${suite_entry##*:}"

        if [[ -x "$suite_script" ]]; then
            run_suite "$suite_name" "$suite_script" || overall_exit=1
        else
            log "SKIP" "Suite $suite_name not found or not executable: $suite_script"
            TOTAL_SKIPPED=$((TOTAL_SKIPPED + 1))
        fi
    done

    # Final summary
    echo ""
    echo "=========================================="
    log "RESULT" "E2E Test Results Summary"
    echo "=========================================="
    log "RESULT" "  Total Passed:  $TOTAL_PASSED"
    log "RESULT" "  Total Failed:  $TOTAL_FAILED"
    log "RESULT" "  Total Skipped: $TOTAL_SKIPPED"
    echo "=========================================="

    # Write summary to log
    cat >> "$LOG_FILE" << EOF

================================================================================
FINAL SUMMARY
================================================================================
Total Passed:  $TOTAL_PASSED
Total Failed:  $TOTAL_FAILED
Total Skipped: $TOTAL_SKIPPED
Overall Exit:  $overall_exit
================================================================================
EOF

    if [[ $overall_exit -eq 0 ]]; then
        log "RESULT" "All tests passed!"
    else
        log "RESULT" "Some tests failed - see $LOG_FILE for details"
    fi

    exit $overall_exit
}

main "$@"
