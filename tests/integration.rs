//! E2E Integration tests for aadc
//!
//! Run with: cargo test --test integration
//! Verbose:  TEST_VERBOSE=1 cargo test --test integration -- --nocapture

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Test logging macro - prints when TEST_VERBOSE is set
macro_rules! test_log {
    ($level:expr, $($arg:tt)*) => {
        if std::env::var("TEST_VERBOSE").is_ok() {
            eprintln!("[{}] [integration:{}] {}",
                $level,
                line!(),
                format!($($arg)*)
            );
        }
    };
}

fn get_binary_path() -> PathBuf {
    // Try release first, then debug
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let release_path = PathBuf::from(manifest_dir).join("target/release/aadc");
    let debug_path = PathBuf::from(manifest_dir).join("target/debug/aadc");

    // Check CARGO_TARGET_DIR override
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let custom_release = PathBuf::from(&target_dir).join("release/aadc");
        let custom_debug = PathBuf::from(&target_dir).join("debug/aadc");
        if custom_release.exists() {
            return custom_release;
        }
        if custom_debug.exists() {
            return custom_debug;
        }
    }

    if release_path.exists() {
        release_path
    } else if debug_path.exists() {
        debug_path
    } else {
        // Fall back to cargo run
        panic!(
            "aadc binary not found. Run 'cargo build' or 'cargo build --release' first.\n\
             Looked in:\n  - {}\n  - {}",
            release_path.display(),
            debug_path.display()
        );
    }
}

fn run_aadc_stdin(input: &str, args: &[&str]) -> (String, String, i32) {
    test_log!("RUN", "aadc with args: {:?}", args);
    test_log!("INPUT", "Input length: {} bytes", input.len());

    let binary = get_binary_path();
    test_log!("BIN", "Using binary: {}", binary.display());

    let mut child = Command::new(&binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn aadc");

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on aadc");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    test_log!("OUTPUT", "Exit code: {}", code);
    test_log!("OUTPUT", "Stdout length: {} bytes", stdout.len());
    if !stderr.is_empty() {
        test_log!("STDERR", "{}", stderr);
    }

    (stdout, stderr, code)
}

fn run_aadc_file(file_path: &str, args: &[&str]) -> (String, String, i32) {
    test_log!("RUN", "aadc {} with args: {:?}", file_path, args);

    let binary = get_binary_path();
    let mut cmd_args: Vec<&str> = args.to_vec();
    cmd_args.push(file_path);

    let output = Command::new(&binary)
        .args(&cmd_args)
        .output()
        .expect("Failed to run aadc");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    test_log!("OUTPUT", "Exit code: {}", code);

    (stdout, stderr, code)
}

// ============================================================================
// Basic Functionality Tests
// ============================================================================

#[test]
fn test_e2e_basic_ascii_correction() {
    test_log!("START", "Basic ASCII box correction");

    let input = "+------------------+
| Short|
| Much longer text |
| Medium|
+------------------+";

    let expected = "+------------------+
| Short            |
| Much longer text |
| Medium           |
+------------------+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should exit successfully");
    assert_eq!(
        stdout.trim(),
        expected.trim(),
        "Output should match expected"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_unicode_box_correction() {
    test_log!("START", "Unicode box correction");

    let input = "┌────────────────┐
│ API Gateway│
│ Authentication │
│ Rate Limiting│
└────────────────┘";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should exit successfully");
    assert!(
        stdout.contains("│ API Gateway    │"),
        "Should pad API Gateway line"
    );
    assert!(
        stdout.contains("│ Rate Limiting  │"),
        "Should pad Rate Limiting line"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_empty_input() {
    test_log!("START", "Empty input handling");

    let (stdout, _stderr, code) = run_aadc_stdin("", &[]);

    assert_eq!(code, 0, "Should exit successfully on empty input");
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "Should produce empty output"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_no_diagrams() {
    test_log!("START", "Text without diagrams passthrough");

    let input = "This is just plain text.\nNo diagrams here.\n";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should exit successfully");
    assert_eq!(stdout, input, "Should pass through unchanged");

    test_log!("END", "Test PASSED");
}

// ============================================================================
// CLI Options Tests
// ============================================================================

#[test]
fn test_e2e_verbose_mode() {
    test_log!("START", "Verbose mode output");

    let input = "+---+
| a|
+---+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &["-v"]);

    assert_eq!(code, 0, "Should exit successfully");
    // Verbose mode should produce output containing the corrected diagram
    assert!(stdout.contains("+---+"), "Should contain box borders");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_min_score_threshold() {
    test_log!("START", "Minimum score threshold option");

    let input = "+---+
| a|
+---+";

    // Very low threshold should accept more revisions
    let (stdout_low, _stderr, code) = run_aadc_stdin(input, &["--min-score", "0.1"]);
    assert_eq!(code, 0, "Low threshold should succeed");
    assert!(!stdout_low.is_empty(), "Should produce output");

    // Very high threshold should reject more revisions
    let (_stdout_high, _stderr, code) = run_aadc_stdin(input, &["--min-score", "0.99"]);
    assert_eq!(code, 0, "High threshold should succeed");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_process_all_flag() {
    test_log!("START", "Process all blocks flag");

    let input = "+---+
| a |
+---+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &["--all"]);

    assert_eq!(code, 0, "Should exit successfully with --all flag");
    assert!(stdout.contains("+---+"), "Should contain the diagram");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_max_iterations() {
    test_log!("START", "Max iterations option");

    let input = "+-------+
| test|
+-------+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &["--max-iters", "3"]);

    assert_eq!(code, 0, "Should exit successfully");
    assert!(stdout.contains("+-------+"), "Should contain the diagram");

    test_log!("END", "Test PASSED");
}

// ============================================================================
// Exit Code Tests
// ============================================================================

#[test]
fn test_e2e_exit_code_success() {
    test_log!("START", "Exit code 0 on success");

    let input = "+---+
| a |
+---+";

    let (_stdout, _stderr, code) = run_aadc_stdin(input, &[]);
    assert_eq!(code, 0, "Should return 0 on success");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_exit_code_dry_run_no_changes() {
    test_log!("START", "Exit code 0 on dry-run with no changes");

    let input = "+---+
| a |
+---+";

    let (_stdout, _stderr, code) = run_aadc_stdin(input, &["-n"]);
    assert_eq!(code, 0, "Should return 0 when no changes needed");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_exit_code_dry_run_would_change() {
    test_log!("START", "Exit code 3 on dry-run when changes would be made");

    let input = "+---+
| a|
+---+";

    let (_stdout, _stderr, code) = run_aadc_stdin(input, &["-n"]);
    assert_eq!(
        code, 3,
        "Should return 3 (WOULD_CHANGE) when changes needed"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_exit_code_nonexistent_file() {
    test_log!("START", "Non-zero exit code for non-existent file");

    let (_stdout, _stderr, code) = run_aadc_file("/nonexistent/path/file.txt", &[]);
    assert_ne!(code, 0, "Should return non-zero for non-existent file");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_exit_code_invalid_utf8() {
    test_log!("START", "Exit code 4 for invalid UTF-8");

    // Create temp file with invalid UTF-8
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("aadc_test_invalid_utf8.bin");
    fs::write(&temp_file, [0xff, 0xfe]).expect("Failed to write temp file");

    let (_stdout, _stderr, code) = run_aadc_file(temp_file.to_str().unwrap(), &[]);

    // Clean up
    let _ = fs::remove_file(&temp_file);

    assert_eq!(code, 4, "Should return 4 (PARSE_ERROR) for invalid UTF-8");

    test_log!("END", "Test PASSED");
}

// ============================================================================
// Diff Mode Tests
// ============================================================================

#[test]
fn test_e2e_diff_mode_with_changes() {
    test_log!("START", "Diff mode with changes");

    let input = "+---+
| a|
+---+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &["--diff"]);

    assert_eq!(code, 0, "Should exit successfully");
    assert!(stdout.contains("--- a/stdin"), "Should contain diff header");
    assert!(stdout.contains("+++ b/stdin"), "Should contain diff header");
    assert!(stdout.contains("-| a|"), "Should show removed line");
    assert!(stdout.contains("+| a |"), "Should show added line");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_diff_mode_no_changes() {
    test_log!("START", "Diff mode with no changes");

    let input = "+---+
| a |
+---+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &["--diff"]);

    assert_eq!(code, 0, "Should exit successfully");
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "Should produce no diff when no changes"
    );

    test_log!("END", "Test PASSED");
}

// ============================================================================
// Edge Cases Tests
// ============================================================================

#[test]
fn test_e2e_cjk_content() {
    test_log!("START", "CJK content handling");

    let input = "┌──────────────────┐
│ Hello 你好│
│ World 世界│
│ Test │
└──────────────────┘";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should handle CJK content successfully");
    assert!(
        stdout.contains("Hello 你好"),
        "Should preserve CJK characters"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_nested_boxes() {
    test_log!("START", "Nested boxes handling");

    let input = "┌───────────────────────────┐
│ Outer box│
│ ┌───────────────────┐│
│ │ Inner box│       │
│ └───────────────────┘│
└───────────────────────────┘";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should handle nested boxes");
    assert!(
        stdout.contains("Outer box"),
        "Should preserve outer box content"
    );
    assert!(
        stdout.contains("Inner box"),
        "Should preserve inner box content"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_multiple_diagrams() {
    test_log!("START", "Multiple diagrams in one file");

    let input = "First diagram:
+---+
| a|
+---+

Second diagram:
+----+
| bb|
+----+";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should handle multiple diagrams");
    assert!(stdout.contains("+---+"), "Should contain first diagram");
    assert!(stdout.contains("+----+"), "Should contain second diagram");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_whitespace_only() {
    test_log!("START", "Whitespace-only input");

    // Note: tabs are expanded based on --tab-width (default 4)
    // So we test with spaces only to avoid tab expansion effects
    let input = "   \n      \n   \n";

    let (stdout, _stderr, code) = run_aadc_stdin(input, &[]);

    assert_eq!(code, 0, "Should handle whitespace-only input");
    // Whitespace-only content should pass through unchanged
    assert_eq!(stdout, input, "Should preserve whitespace");

    test_log!("END", "Test PASSED");
}

// ============================================================================
// Multiple Files Tests (new feature from bd-5jn)
// ============================================================================

#[test]
fn test_e2e_multiple_files() {
    test_log!("START", "Multiple file input");

    // Create temp files
    let temp_dir = std::env::temp_dir();
    let file1 = temp_dir.join("aadc_test_multi1.txt");
    let file2 = temp_dir.join("aadc_test_multi2.txt");

    fs::write(
        &file1,
        "+---+
| a|
+---+",
    )
    .expect("Failed to write temp file 1");

    fs::write(
        &file2,
        "+----+
| bb|
+----+",
    )
    .expect("Failed to write temp file 2");

    let binary = get_binary_path();
    let output = Command::new(&binary)
        .arg(file1.to_str().unwrap())
        .arg(file2.to_str().unwrap())
        .output()
        .expect("Failed to run aadc");

    // Clean up
    let _ = fs::remove_file(&file1);
    let _ = fs::remove_file(&file2);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let code = output.status.code().unwrap_or(-1);

    assert_eq!(code, 0, "Should process multiple files successfully");
    // Should contain output from both files with headers
    assert!(
        stdout.contains("==>") || stdout.contains("+---+"),
        "Should contain output from files"
    );

    test_log!("END", "Test PASSED");
}

// ============================================================================
// Error Handling Tests (from bd-b9s)
// ============================================================================

#[test]
fn test_e2e_exit_code_invalid_tab_width_zero() {
    test_log!("START", "Exit code 2 for invalid tab width (0)");

    let input = "+---+\n| a |\n+---+";
    let (_stdout, stderr, code) = run_aadc_stdin(input, &["--tab-width", "0"]);

    assert_eq!(code, 2, "Should return 2 (INVALID_ARGS) for tab-width=0");
    assert!(
        stderr.contains("--tab-width must be between 1 and 16"),
        "Error message should mention valid range"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_exit_code_invalid_tab_width_too_large() {
    test_log!("START", "Exit code 2 for invalid tab width (17)");

    let input = "+---+\n| a |\n+---+";
    let (_stdout, stderr, code) = run_aadc_stdin(input, &["--tab-width", "17"]);

    assert_eq!(code, 2, "Should return 2 (INVALID_ARGS) for tab-width=17");
    assert!(
        stderr.contains("--tab-width must be between 1 and 16"),
        "Error message should mention valid range"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_valid_tab_width_edge_cases() {
    test_log!("START", "Valid tab width edge cases (1 and 16)");

    let input = "+---+\n| a |\n+---+";

    // Tab width 1 should work
    let (_stdout, _stderr, code) = run_aadc_stdin(input, &["--tab-width", "1"]);
    assert_eq!(code, 0, "Tab width 1 should be valid");

    // Tab width 16 should work
    let (_stdout, _stderr, code) = run_aadc_stdin(input, &["--tab-width", "16"]);
    assert_eq!(code, 0, "Tab width 16 should be valid");

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_high_max_iters_warning() {
    test_log!("START", "Warning for high max-iters");

    let input = "+---+\n| a |\n+---+";
    let (_stdout, stderr, code) = run_aadc_stdin(input, &["--max-iters", "500"]);

    assert_eq!(code, 0, "Should still succeed with high max-iters");
    assert!(
        stderr.contains("Warning") && stderr.contains("500"),
        "Should warn about high max-iters value"
    );

    test_log!("END", "Test PASSED");
}

#[test]
fn test_e2e_binary_file_detection() {
    test_log!("START", "Binary file detection with null bytes");

    // Create temp file with null bytes (binary indicator)
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("aadc_test_binary.bin");
    fs::write(&temp_file, b"+---+\0| a |\0+---+").expect("Failed to write temp file");

    let (_stdout, stderr, code) = run_aadc_file(temp_file.to_str().unwrap(), &[]);

    // Clean up
    let _ = fs::remove_file(&temp_file);

    assert_eq!(code, 4, "Should return 4 (PARSE_ERROR) for binary input");
    assert!(
        stderr.contains("binary"),
        "Error message should mention binary"
    );

    test_log!("END", "Test PASSED");
}
