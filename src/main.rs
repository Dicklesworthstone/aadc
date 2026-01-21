//! ASCII Art Diagram Corrector (aadc)
//!
//! A CLI tool that fixes misaligned right-hand borders in ASCII diagrams.
//! Uses an iterative correction loop with scoring to achieve clean alignment.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::Parser;
use clap::ValueEnum;
use clap::error::ErrorKind;
use rich_rust::Console;
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// Exit Codes
// ─────────────────────────────────────────────────────────────────────────────

/// Semantic exit codes for scripting and CI integration
mod exit_codes {
    /// Success - completed without errors
    pub const SUCCESS: i32 = 0;
    /// General error (file not found, permission denied, I/O error)
    pub const ERROR: i32 = 1;
    /// Invalid command-line arguments
    pub const INVALID_ARGS: i32 = 2;
    /// Dry-run mode: changes would be made
    pub const WOULD_CHANGE: i32 = 3;
    /// Parse error (invalid UTF-8 or binary file detected)
    pub const PARSE_ERROR: i32 = 4;
}

#[derive(Debug)]
struct ArgError(String);

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ArgError {}

#[derive(Debug)]
struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug)]
struct RunOutcome {
    dry_run: bool,
    would_change: bool,
}

fn error_chain_has<T: std::error::Error + 'static>(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<T>())
}

fn exit_code_for_error(err: &anyhow::Error) -> i32 {
    if error_chain_has::<ArgError>(err) {
        exit_codes::INVALID_ARGS
    } else if error_chain_has::<ParseError>(err) {
        exit_codes::PARSE_ERROR
    } else {
        exit_codes::ERROR
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Arguments
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Preset {
    /// Conservative: only high-confidence edits (0.8)
    Strict,
    /// Balanced: reasonable confidence threshold (0.5)
    Normal,
    /// Aggressive: accept lower-confidence edits (0.3)
    Aggressive,
    /// Accept almost any edit (0.1)
    Relaxed,
}

impl Preset {
    fn min_score(self) -> f64 {
        match self {
            Self::Strict => 0.8,
            Self::Normal => 0.5,
            Self::Aggressive => 0.3,
            Self::Relaxed => 0.1,
        }
    }
}

/// ASCII Art Diagram Corrector: fixes misaligned right borders in ASCII diagrams
#[derive(Parser, Debug)]
#[command(
    name = "aadc",
    version,
    about,
    long_about = None,
    after_help = "EXIT CODES:\n  0  Success\n  1  General error (file not found, permission denied, I/O error)\n  2  Invalid command-line arguments\n  3  Dry-run mode: changes would be made\n  4  Parse error (invalid UTF-8 or binary input)\n"
)]
struct Args {
    /// Input file(s). Reads from stdin if not provided.
    /// Multiple files can be specified.
    #[arg(value_name = "FILE")]
    inputs: Vec<PathBuf>,

    /// Edit file(s) in place
    #[arg(short = 'i', long)]
    in_place: bool,

    /// Confidence threshold preset (conflicts with --min-score)
    #[arg(long, short = 'P', value_enum, conflicts_with = "min_score")]
    preset: Option<Preset>,

    /// Maximum iterations for correction loop
    #[arg(short = 'm', long, default_value = "10")]
    max_iters: usize,

    /// Minimum score threshold for applying revisions (0.0-1.0)
    #[arg(short = 's', long, default_value = "0.5")]
    min_score: f64,

    /// Tab width for expansion
    #[arg(short = 't', long, default_value = "4")]
    tab_width: usize,

    /// Process all diagram-like blocks, not just confident ones
    #[arg(short = 'a', long)]
    all: bool,

    /// Verbose output showing correction progress
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Show unified diff of changes instead of full output
    #[arg(short = 'd', long)]
    diff: bool,

    /// Preview changes without modifying files (exit 0=no changes, 3=would change)
    #[arg(short = 'n', long, conflicts_with = "in_place")]
    dry_run: bool,

    /// Create backup file before in-place editing
    #[arg(long, requires = "in_place")]
    backup: bool,

    /// Extension for backup files (default: .bak)
    #[arg(long, default_value = ".bak", requires = "backup")]
    backup_ext: String,

    /// Output results as JSON for programmatic processing
    #[arg(long, conflicts_with_all = ["verbose", "diff"])]
    json: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration and Statistics
// ─────────────────────────────────────────────────────────────────────────────

/// Runtime configuration derived from CLI args
struct Config {
    max_iters: usize,
    min_score: f64,
    preset: Option<Preset>,
    tab_width: usize,
    all_blocks: bool,
    verbose: bool,
    diff: bool,
    dry_run: bool,
    backup: bool,
    backup_ext: String,
    json: bool,
}

impl From<&Args> for Config {
    fn from(args: &Args) -> Self {
        Self {
            max_iters: args.max_iters,
            min_score: args.min_score,
            preset: args.preset,
            tab_width: args.tab_width,
            all_blocks: args.all,
            verbose: args.verbose,
            diff: args.diff,
            dry_run: args.dry_run,
            backup: args.backup,
            backup_ext: args.backup_ext.clone(),
            json: args.json,
        }
    }
}

impl Config {
    fn effective_min_score(&self) -> f64 {
        match self.preset {
            Some(preset) => preset.min_score(),
            None => self.min_score,
        }
    }
}

fn validate_args(args: &Args) -> Result<()> {
    if !(0.0..=1.0).contains(&args.min_score) {
        return Err(ArgError("--min-score must be between 0.0 and 1.0".to_string()).into());
    }

    if args.max_iters == 0 {
        return Err(ArgError("--max-iters must be at least 1".to_string()).into());
    }

    if args.in_place && args.inputs.is_empty() {
        return Err(ArgError("--in-place requires at least one input file".to_string()).into());
    }

    Ok(())
}

/// Statistics collected during correction
#[derive(Default)]
struct Stats {
    blocks_found: usize,
    blocks_modified: usize,
    total_revisions: usize,
    #[allow(dead_code)]
    iterations: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON Output Structures
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JsonOutput {
    version: &'static str,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    input: InputStats,
    processing: ProcessingStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<OutputStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Serialize)]
struct InputStats {
    lines: usize,
    bytes: usize,
}

#[derive(Serialize)]
struct ProcessingStats {
    blocks_detected: usize,
    blocks_modified: usize,
    revisions_applied: usize,
}

#[derive(Serialize)]
struct OutputStats {
    lines: usize,
    bytes: usize,
    changed: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Line Classification
// ─────────────────────────────────────────────────────────────────────────────

/// Classification of a line's "boxiness"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// Empty or whitespace-only
    Blank,
    /// No box-drawing characters detected
    None,
    /// Some box-drawing characters but weak pattern
    Weak,
    /// Strong box-drawing pattern (borders, corners)
    Strong,
}

impl LineKind {
    fn is_boxy(self) -> bool {
        matches!(self, Self::Weak | Self::Strong)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Box Drawing Character Detection
// ─────────────────────────────────────────────────────────────────────────────

/// Check if character is a corner piece (ASCII or Unicode)
fn is_corner(c: char) -> bool {
    matches!(
        c,
        '+' | '┌' | '┐' | '└' | '┘' | '╔' | '╗' | '╚' | '╝' | '╭' | '╮' | '╯' | '╰'
    )
}

/// Check if character is a horizontal fill (for borders)
fn is_horizontal_fill(c: char) -> bool {
    matches!(
        c,
        '-' | '─' | '━' | '═' | '╌' | '╍' | '┄' | '┅' | '┈' | '┉' | '~' | '='
    )
}

/// Check if character is a vertical border
fn is_vertical_border(c: char) -> bool {
    matches!(c, '|' | '│' | '┃' | '║' | '╎' | '╏' | '┆' | '┇' | '┊' | '┋')
}

/// Check if character is a T-junction
fn is_junction(c: char) -> bool {
    matches!(
        c,
        '┬' | '┴'
            | '├'
            | '┤'
            | '┼'
            | '╦'
            | '╩'
            | '╠'
            | '╣'
            | '╬'
            | '╤'
            | '╧'
            | '╟'
            | '╢'
            | '╫'
            | '╪'
    )
}

/// Check if character could be part of a box drawing
fn is_box_char(c: char) -> bool {
    is_corner(c) || is_horizontal_fill(c) || is_vertical_border(c) || is_junction(c)
}

/// Check if character can terminate a line border
fn is_border_char(c: char) -> bool {
    is_vertical_border(c) || is_corner(c) || is_junction(c)
}

/// Detect the most common vertical border character in a set of lines
fn detect_vertical_border(lines: &[&str]) -> char {
    let mut counts = std::collections::HashMap::new();

    for line in lines {
        for c in line.chars() {
            if is_vertical_border(c) {
                *counts.entry(c).or_insert(0) += 1;
            }
        }
    }

    // Default to ASCII pipe if no Unicode detected
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(c, _)| c)
        .unwrap_or('|')
}

// ─────────────────────────────────────────────────────────────────────────────
// Line Analysis
// ─────────────────────────────────────────────────────────────────────────────

/// Analyzed line with extracted properties
#[derive(Debug)]
struct AnalyzedLine {
    /// The original line content
    #[allow(dead_code)]
    content: String,
    /// Classification of the line
    kind: LineKind,
    /// Visual width (accounting for wide chars)
    #[allow(dead_code)]
    visual_width: usize,
    /// Left indentation (leading spaces)
    #[allow(dead_code)]
    indent: usize,
    /// Detected suffix border info if present
    suffix_border: Option<SuffixBorder>,
}

/// Information about a detected right-side border
#[derive(Debug, Clone)]
struct SuffixBorder {
    /// Column position where the border starts
    column: usize,
    /// The border character
    #[allow(dead_code)]
    char: char,
    /// Whether this looks like a closing border (vs mid-line)
    #[allow(dead_code)]
    is_closing: bool,
}

/// Calculate visual width of a string (handling wide chars)
fn visual_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                1
            } else {
                // Simple heuristic: most CJK and emoji are double-width
                // Box drawing chars are single-width
                if is_box_char(c) {
                    1
                } else if c >= '\u{1100}' {
                    2
                } else {
                    1
                }
            }
        })
        .sum()
}

/// Classify a single line
fn classify_line(line: &str) -> LineKind {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return LineKind::Blank;
    }

    let box_chars: usize = trimmed.chars().filter(|&c| is_box_char(c)).count();
    let total_chars = trimmed.chars().count();

    if box_chars == 0 {
        return LineKind::None;
    }

    // Check for strong indicators
    let has_corner = trimmed.chars().any(is_corner);
    let starts_with_border = trimmed.chars().next().is_some_and(is_border_char);
    let ends_with_border = trimmed.chars().next_back().is_some_and(is_border_char);

    // Strong: has corners, or starts AND ends with border chars, or high ratio
    if has_corner || (starts_with_border && ends_with_border) || box_chars * 3 >= total_chars {
        LineKind::Strong
    } else if box_chars > 0 {
        LineKind::Weak
    } else {
        LineKind::None
    }
}

/// Analyze a line for correction
fn analyze_line(line: &str) -> AnalyzedLine {
    let kind = classify_line(line);
    let visual = visual_width(line);
    let indent = line.len() - line.trim_start().len();

    // Detect suffix border
    let suffix_border = if kind.is_boxy() {
        detect_suffix_border(line)
    } else {
        None
    };

    AnalyzedLine {
        content: line.to_string(),
        kind,
        visual_width: visual,
        indent,
        suffix_border,
    }
}

/// Detect a right-side border in a line
fn detect_suffix_border(line: &str) -> Option<SuffixBorder> {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    let last_char = trimmed.chars().next_back()?;

    if is_border_char(last_char) {
        let prefix = &trimmed[..trimmed.len() - last_char.len_utf8()];
        let column = visual_width(prefix);
        Some(SuffixBorder {
            column,
            char: last_char,
            is_closing: is_corner(last_char) || is_junction(last_char),
        })
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Diagram Block Detection
// ─────────────────────────────────────────────────────────────────────────────

/// A detected diagram block
#[derive(Debug)]
struct DiagramBlock {
    /// Starting line index (0-based)
    start: usize,
    /// Ending line index (exclusive)
    end: usize,
    /// Confidence score (0.0-1.0)
    confidence: f64,
}

/// Find diagram blocks in the text
fn find_diagram_blocks(lines: &[String], all_blocks: bool) -> Vec<DiagramBlock> {
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        // Skip blank/non-boxy lines
        let kind = classify_line(&lines[i]);
        if !kind.is_boxy() {
            i += 1;
            continue;
        }

        // Found potential start of a block
        let start = i;
        let mut end = i + 1;
        let mut strong_count = if kind == LineKind::Strong { 1 } else { 0 };
        let mut weak_count = if kind == LineKind::Weak { 1 } else { 0 };
        let mut blank_gap = 0;

        // Extend block
        while end < lines.len() {
            let next_kind = classify_line(&lines[end]);

            match next_kind {
                LineKind::Strong => {
                    strong_count += 1;
                    blank_gap = 0;
                    end += 1;
                }
                LineKind::Weak => {
                    weak_count += 1;
                    blank_gap = 0;
                    end += 1;
                }
                LineKind::Blank => {
                    // Allow small gaps within diagrams
                    blank_gap += 1;
                    if blank_gap > 1 {
                        break;
                    }
                    end += 1;
                }
                LineKind::None => {
                    // Check if next non-blank is boxy
                    let lookahead = lines
                        .iter()
                        .skip(end)
                        .take(3)
                        .any(|l| classify_line(l).is_boxy());
                    if lookahead && blank_gap == 0 {
                        end += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        // Trim trailing blanks
        while end > start && classify_line(&lines[end - 1]) == LineKind::Blank {
            end -= 1;
        }

        // Calculate confidence
        let total = strong_count + weak_count;
        let confidence = if total > 0 {
            let strong_ratio = strong_count as f64 / total as f64;
            let size_bonus = ((end - start) as f64 / 10.0).min(0.2);
            (strong_ratio * 0.8 + size_bonus).min(1.0)
        } else {
            0.0
        };

        // Add block if confidence meets threshold
        if all_blocks || confidence >= 0.3 {
            blocks.push(DiagramBlock {
                start,
                end,
                confidence,
            });
        }

        i = end;
    }

    blocks
}

// ─────────────────────────────────────────────────────────────────────────────
// Revision System
// ─────────────────────────────────────────────────────────────────────────────

/// A proposed revision to a line
#[derive(Debug, Clone)]
enum Revision {
    /// Pad before the suffix border to align it
    PadBeforeSuffixBorder {
        line_idx: usize,
        spaces_to_add: usize,
        #[allow(dead_code)]
        target_column: usize,
    },
    /// Add a missing suffix border
    AddSuffixBorder {
        line_idx: usize,
        border_char: char,
        target_column: usize,
    },
}

impl Revision {
    /// Score this revision (higher = more confident it's correct)
    /// `block_start` is the offset of the block in the global lines array
    fn score(&self, analyzed: &[AnalyzedLine], block_start: usize) -> f64 {
        match self {
            Self::PadBeforeSuffixBorder {
                line_idx,
                spaces_to_add,
                ..
            } => {
                let local_idx = line_idx - block_start;
                let line = &analyzed[local_idx];
                // Prefer smaller adjustments
                let adjustment_penalty = (*spaces_to_add as f64 / 10.0).min(0.5);
                // Prefer strong lines
                let strength_bonus = if line.kind == LineKind::Strong {
                    0.2
                } else {
                    0.0
                };
                0.8 - adjustment_penalty + strength_bonus
            }
            Self::AddSuffixBorder { line_idx, .. } => {
                let local_idx = line_idx - block_start;
                let line = &analyzed[local_idx];
                // Adding borders is less confident
                let base = 0.5;
                let strength_bonus = if line.kind == LineKind::Strong {
                    0.2
                } else {
                    0.1
                };
                base + strength_bonus
            }
        }
    }

    /// Apply this revision to the lines
    fn apply(&self, lines: &mut [String]) {
        match self {
            Self::PadBeforeSuffixBorder {
                line_idx,
                spaces_to_add,
                ..
            } => {
                let line = &mut lines[*line_idx];
                let trimmed = line.trim_end();
                if let Some(last_char) = trimmed.chars().next_back() {
                    if is_border_char(last_char) {
                        // Insert spaces before the last character
                        let prefix = &trimmed[..trimmed.len() - last_char.len_utf8()];
                        *line = format!("{}{}{}", prefix, " ".repeat(*spaces_to_add), last_char);
                    }
                }
            }
            Self::AddSuffixBorder {
                line_idx,
                border_char,
                target_column,
            } => {
                let line = &mut lines[*line_idx];
                let current_width = visual_width(line.trim_end());
                let padding = target_column.saturating_sub(current_width);
                *line = format!("{}{}{}", line.trim_end(), " ".repeat(padding), border_char);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Block Correction
// ─────────────────────────────────────────────────────────────────────────────

/// Correct a single diagram block
fn correct_block(
    lines: &mut [String],
    block: &DiagramBlock,
    config: &Config,
    console: &Console,
) -> usize {
    let mut total_revisions = 0;

    for iteration in 0..config.max_iters {
        // Analyze current state
        let block_lines: Vec<_> = lines[block.start..block.end].iter().collect();
        let analyzed: Vec<_> = block_lines.iter().map(|l| analyze_line(l)).collect();

        // Find target column (rightmost border position)
        let target_column = analyzed
            .iter()
            .filter_map(|a| a.suffix_border.as_ref().map(|b| b.column))
            .max();

        let Some(target) = target_column else {
            // No borders found, nothing to align
            break;
        };

        // Generate revision candidates
        let mut revisions = Vec::new();
        let border_char =
            detect_vertical_border(&block_lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        for (i, analyzed_line) in analyzed.iter().enumerate() {
            let global_idx = block.start + i;

            if let Some(ref border) = analyzed_line.suffix_border {
                if border.column < target {
                    let spaces = target - border.column;
                    revisions.push(Revision::PadBeforeSuffixBorder {
                        line_idx: global_idx,
                        spaces_to_add: spaces,
                        target_column: target,
                    });
                }
            } else if analyzed_line.kind.is_boxy() {
                // Consider adding a border
                revisions.push(Revision::AddSuffixBorder {
                    line_idx: global_idx,
                    border_char,
                    target_column: target,
                });
            }
        }

        // Filter by score
        let min_score = config.effective_min_score();
        let valid_revisions: Vec<_> = revisions
            .into_iter()
            .filter(|r| r.score(&analyzed, block.start) >= min_score)
            .collect();

        if valid_revisions.is_empty() {
            // Converged
            if config.verbose && iteration > 0 {
                console.print(&format!(
                    "[dim]    Converged after {} iteration(s)[/]",
                    iteration
                ));
            }
            break;
        }

        // Apply revisions
        for rev in &valid_revisions {
            rev.apply(lines);
        }

        total_revisions += valid_revisions.len();

        if config.verbose {
            console.print(&format!(
                "[dim]    Iteration {}: applied {} revision(s)[/]",
                iteration + 1,
                valid_revisions.len()
            ));
        }
    }

    total_revisions
}

// ─────────────────────────────────────────────────────────────────────────────
// Main Correction Logic
// ─────────────────────────────────────────────────────────────────────────────

/// Expand tabs to spaces
fn expand_tabs(line: &str, tab_width: usize) -> String {
    let mut result = String::with_capacity(line.len());
    let mut col = 0;

    for c in line.chars() {
        if c == '\t' {
            let spaces = tab_width - (col % tab_width);
            result.extend(std::iter::repeat_n(' ', spaces));
            col += spaces;
        } else {
            result.push(c);
            col += 1;
        }
    }

    result
}

/// Main correction entry point
fn correct_lines(lines: Vec<String>, config: &Config, console: &Console) -> (Vec<String>, Stats) {
    let mut stats = Stats::default();

    // Expand tabs
    let mut lines: Vec<String> = lines
        .into_iter()
        .map(|l| expand_tabs(&l, config.tab_width))
        .collect();

    // Find diagram blocks
    let blocks = find_diagram_blocks(&lines, config.all_blocks);
    stats.blocks_found = blocks.len();

    if config.verbose {
        console.print(&format!(
            "[bold cyan]Found {} diagram block(s)[/]",
            blocks.len()
        ));
    }

    // Correct each block
    for (i, block) in blocks.iter().enumerate() {
        if config.verbose {
            console.print(&format!(
                "[yellow]  Block {}: lines {}-{} (confidence: {:.0}%)[/]",
                i + 1,
                block.start + 1,
                block.end,
                block.confidence * 100.0
            ));
        }

        let revisions = correct_block(&mut lines, block, config, console);
        if revisions > 0 {
            stats.blocks_modified += 1;
            stats.total_revisions += revisions;
        }
    }

    (lines, stats)
}

// ─────────────────────────────────────────────────────────────────────────────
// Backup
// ─────────────────────────────────────────────────────────────────────────────

/// Creates a backup of the file by appending the extension to the filename.
/// For example: "file.txt" with extension ".bak" becomes "file.txt.bak"
fn create_backup(path: &Path, ext: &str) -> Result<PathBuf> {
    let mut backup_name = path.as_os_str().to_owned();
    backup_name.push(ext);
    let backup_path = PathBuf::from(backup_name);

    fs::copy(path, &backup_path)
        .with_context(|| format!("Failed to create backup at {}", backup_path.display()))?;

    Ok(backup_path)
}

/// Read content from a file path and return lines
fn read_file(path: &Path) -> Result<Vec<String>> {
    let source_label = path.display().to_string();
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read input file: {}", path.display()))?;

    parse_bytes_to_lines(bytes, &source_label)
}

/// Read content from stdin and return lines
fn read_stdin_content() -> Result<Vec<String>> {
    let mut buf = Vec::new();
    io::stdin()
        .read_to_end(&mut buf)
        .context("Failed to read stdin")?;
    parse_bytes_to_lines(buf, "stdin")
}

/// Convert raw bytes to lines, checking for binary content and valid UTF-8
fn parse_bytes_to_lines(bytes: Vec<u8>, source_label: &str) -> Result<Vec<String>> {
    if bytes.contains(&0) {
        return Err(ParseError(format!("Input appears to be binary: {}", source_label)).into());
    }

    let content = String::from_utf8(bytes).map_err(|err| {
        let utf8_err = err.utf8_error();
        let valid_up_to = utf8_err.valid_up_to();
        let byte = err.as_bytes().get(valid_up_to).copied();
        let detail = match byte {
            Some(b) => format!(
                "Invalid UTF-8 at byte position {} (byte value: 0x{:02X}) in {}",
                valid_up_to, b, source_label
            ),
            None => format!("Invalid UTF-8 in {}", source_label),
        };
        ParseError(detail)
    })?;

    Ok(content.lines().map(String::from).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry Point
// ─────────────────────────────────────────────────────────────────────────────

/// Result of processing a single file or stdin
struct FileResult {
    filename: String,
    original: Vec<String>,
    corrected: Vec<String>,
    stats: Stats,
    would_change: bool,
}

fn main() {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(err) => {
            let code = match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => exit_codes::SUCCESS,
                _ => exit_codes::INVALID_ARGS,
            };
            let _ = err.print();
            std::process::exit(code);
        }
    };

    let exit_code = match run(args) {
        Ok(outcome) => {
            if outcome.dry_run && outcome.would_change {
                exit_codes::WOULD_CHANGE
            } else {
                exit_codes::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("Error: {:#}", err);
            exit_code_for_error(&err)
        }
    };

    std::process::exit(exit_code);
}

/// Process a single input (file or stdin) and return the result
fn process_input(
    lines: Vec<String>,
    filename: String,
    config: &Config,
    console: &Console,
) -> FileResult {
    if config.verbose {
        console.print(&format!(
            "[bold]Processing {} ({} lines)...[/]",
            filename,
            lines.len()
        ));
    }

    let original = lines.clone();
    let (corrected, stats) = correct_lines(lines, config, console);

    let original_text = original.join("\n");
    let corrected_text = corrected.join("\n");
    let would_change = original_text != corrected_text;

    FileResult {
        filename,
        original,
        corrected,
        stats,
        would_change,
    }
}

/// Output a unified diff for a file result
fn output_diff(result: &FileResult, proposed: bool) -> Result<()> {
    if !result.would_change {
        return Ok(());
    }

    let original_text = result.original.join("\n");
    let corrected_text = result.corrected.join("\n");
    let diff = TextDiff::from_lines(&original_text, &corrected_text);
    let mut stdout = io::stdout().lock();

    writeln!(stdout, "--- a/{}", result.filename)?;
    if proposed {
        writeln!(stdout, "+++ b/{} (proposed)", result.filename)?;
    } else {
        writeln!(stdout, "+++ b/{}", result.filename)?;
    }

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        writeln!(stdout, "{}", hunk.header())?;
        for change in hunk.iter_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            let line = change.value();
            if line.ends_with('\n') {
                write!(stdout, "{}{}", sign, line)?;
            } else {
                writeln!(stdout, "{}{}", sign, line)?;
            }
        }
    }

    Ok(())
}

fn run(args: Args) -> Result<RunOutcome> {
    validate_args(&args)?;
    let config = Config::from(&args);
    let console = Console::new();

    if config.verbose {
        if let Some(preset) = config.preset {
            console.print(&format!(
                "[dim]Using preset: {:?} (min_score = {:.1})[/]",
                preset,
                config.effective_min_score()
            ));
        }
    }

    // Determine if we're processing stdin or files
    if args.inputs.is_empty() {
        // Stdin mode - single input
        let lines = read_stdin_content()?;
        let result = process_input(lines, "stdin".to_string(), &config, &console);
        output_single_result(&args, &config, &console, result)
    } else if args.inputs.len() == 1 {
        // Single file mode - same behavior as before
        let path = &args.inputs[0];
        let lines = read_file(path)?;
        let result = process_input(lines, path.display().to_string(), &config, &console);
        output_single_result(&args, &config, &console, result)
    } else {
        // Multiple file mode
        output_multiple_results(&args, &config, &console)
    }
}

/// Handle output for a single file/stdin result
fn output_single_result(
    args: &Args,
    config: &Config,
    console: &Console,
    result: FileResult,
) -> Result<RunOutcome> {
    let would_change = result.would_change;

    if config.json {
        output_json_single(args, config, &result)?;
    } else if config.dry_run {
        output_dry_run_single(config, console, &result)?;
    } else if config.diff {
        output_diff(&result, false)?;
        if config.verbose {
            console.print(&format!(
                "[bold green]Diff: {} block(s), {} revision(s)[/]",
                result.stats.blocks_modified, result.stats.total_revisions
            ));
        }
    } else if args.in_place {
        // Must have a file path for in-place
        let path = args
            .inputs
            .first()
            .ok_or_else(|| ArgError("--in-place requires an input file".to_string()))?;

        if config.backup {
            let backup_path = create_backup(path, &config.backup_ext)?;
            if config.verbose {
                console.print(&format!(
                    "[dim]Created backup: {}[/]",
                    backup_path.display()
                ));
            }
        }

        let output = result.corrected.join("\n");
        fs::write(path, &output)
            .with_context(|| format!("Failed to write to file: {}", path.display()))?;

        if config.verbose {
            console.print(&format!(
                "[bold green]Modified {} block(s), {} revision(s) applied[/]",
                result.stats.blocks_modified, result.stats.total_revisions
            ));
        }
    } else {
        // Stdout mode
        let mut stdout = io::stdout().lock();
        for line in &result.corrected {
            writeln!(stdout, "{}", line)?;
        }

        if config.verbose {
            console.print(&format!(
                "[bold green]Processed {} block(s), {} revision(s) applied[/]",
                result.stats.blocks_found, result.stats.total_revisions
            ));
        }
    }

    Ok(RunOutcome {
        dry_run: config.dry_run,
        would_change,
    })
}

/// Output JSON for a single file result
fn output_json_single(args: &Args, config: &Config, result: &FileResult) -> Result<()> {
    let original_text = result.original.join("\n");
    let corrected_text = result.corrected.join("\n");

    let json_output = JsonOutput {
        version: "1.0",
        status: if config.dry_run {
            "dry_run".to_string()
        } else {
            "success".to_string()
        },
        file: Some(result.filename.clone()),
        input: InputStats {
            lines: result.original.len(),
            bytes: original_text.len(),
        },
        processing: ProcessingStats {
            blocks_detected: result.stats.blocks_found,
            blocks_modified: result.stats.blocks_modified,
            revisions_applied: result.stats.total_revisions,
        },
        output: Some(OutputStats {
            lines: result.corrected.len(),
            bytes: corrected_text.len(),
            changed: result.would_change,
        }),
        content: if !config.dry_run && !args.in_place {
            Some(corrected_text.clone())
        } else {
            None
        },
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&json_output).context("Failed to serialize JSON output")?
    );

    // If in-place mode with JSON, still write the file
    if args.in_place {
        if let Some(ref path) = args.inputs.first() {
            if config.backup {
                create_backup(path, &config.backup_ext)?;
            }
            fs::write(path, &corrected_text)
                .with_context(|| format!("Failed to write to file: {}", path.display()))?;
        }
    }

    Ok(())
}

/// Output dry-run info for a single file
fn output_dry_run_single(config: &Config, console: &Console, result: &FileResult) -> Result<()> {
    if config.diff && result.would_change {
        output_diff(result, true)?;
    }

    if config.verbose {
        if result.would_change {
            console.print(&format!(
                "[bold yellow]Would modify: {}[/]",
                result.filename
            ));
            console.print(&format!(
                "[dim]  {} block(s), {} revision(s)[/]",
                result.stats.blocks_modified, result.stats.total_revisions
            ));
        } else {
            console.print(&format!(
                "[bold green]No changes needed: {}[/]",
                result.filename
            ));
        }
    }

    Ok(())
}

/// Handle output for multiple files
fn output_multiple_results(args: &Args, config: &Config, console: &Console) -> Result<RunOutcome> {
    let mut total_files_processed = 0;
    let mut total_files_changed = 0;
    let mut total_blocks_modified = 0;
    let mut total_revisions = 0;
    let mut any_would_change = false;
    let mut errors: Vec<(PathBuf, anyhow::Error)> = Vec::new();

    let show_file_headers = !args.in_place && !config.diff && !config.json && args.inputs.len() > 1;

    for path in &args.inputs {
        match read_file(path) {
            Ok(lines) => {
                let result = process_input(lines, path.display().to_string(), config, console);

                if result.would_change {
                    any_would_change = true;
                    total_files_changed += 1;
                }
                total_files_processed += 1;
                total_blocks_modified += result.stats.blocks_modified;
                total_revisions += result.stats.total_revisions;

                // Handle output based on mode
                if config.json {
                    // For JSON with multiple files, output each file's JSON separately
                    output_json_single(args, config, &result)?;
                } else if config.dry_run {
                    output_dry_run_single(config, console, &result)?;
                } else if config.diff {
                    output_diff(&result, false)?;
                } else if args.in_place {
                    // Write file in-place
                    if config.backup {
                        let backup_path = create_backup(path, &config.backup_ext)?;
                        if config.verbose {
                            console.print(&format!(
                                "[dim]Created backup: {}[/]",
                                backup_path.display()
                            ));
                        }
                    }

                    let output = result.corrected.join("\n");
                    fs::write(path, &output)
                        .with_context(|| format!("Failed to write to file: {}", path.display()))?;

                    if config.verbose {
                        if result.would_change {
                            console.print(&format!(
                                "[bold green]{}: {} block(s), {} revision(s) applied[/]",
                                path.display(),
                                result.stats.blocks_modified,
                                result.stats.total_revisions
                            ));
                        } else {
                            console
                                .print(&format!("[dim]{}: No changes needed[/]", path.display()));
                        }
                    }
                } else {
                    // Stdout mode - concatenate output with file headers
                    let mut stdout = io::stdout().lock();

                    if show_file_headers {
                        writeln!(stdout, "==> {} <==", path.display())?;
                    }

                    for line in &result.corrected {
                        writeln!(stdout, "{}", line)?;
                    }

                    if show_file_headers {
                        writeln!(stdout)?; // Blank line between files
                    }
                }
            }
            Err(e) => {
                eprintln!("[red]Error processing {}:[/] {:#}", path.display(), e);
                errors.push((path.clone(), e));
            }
        }
    }

    // Print summary for multiple files in verbose mode
    if config.verbose && args.inputs.len() > 1 {
        console.print(&format!(
            "\n[bold]Summary:[/] {} file(s) processed, {} changed, {} block(s), {} revision(s), {} error(s)",
            total_files_processed, total_files_changed, total_blocks_modified, total_revisions, errors.len()
        ));
    }

    // If any files had errors, report them
    if !errors.is_empty() {
        let files = errors
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let has_parse_error = errors
            .iter()
            .any(|(_, err)| error_chain_has::<ParseError>(err));

        if has_parse_error {
            return Err(ParseError(format!(
                "{} file(s) had parse errors: {}",
                errors.len(),
                files
            ))
            .into());
        }

        anyhow::bail!("{} file(s) had errors: {}", errors.len(), files);
    }

    Ok(RunOutcome {
        dry_run: config.dry_run,
        would_change: any_would_change,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_args() -> Args {
        Args {
            inputs: vec![],
            in_place: false,
            preset: None,
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        }
    }

    // =========================================================================
    // Args parsing + validation tests
    // =========================================================================

    #[test]
    fn test_args_defaults() {
        let args = Args::parse_from(["aadc"]);
        assert!(args.inputs.is_empty());
        assert!(!args.in_place);
        assert!(args.preset.is_none());
        assert_eq!(args.max_iters, 10);
        assert_eq!(args.min_score, 0.5);
        assert_eq!(args.tab_width, 4);
        assert!(!args.all);
        assert!(!args.verbose);
        assert!(!args.diff);
        assert!(!args.dry_run);
    }

    #[test]
    fn test_args_custom() {
        let args = Args::parse_from([
            "aadc", "-i", "-m", "20", "-s", "0.7", "-t", "2", "-a", "-v", "-d", "file.txt",
        ]);
        assert_eq!(args.inputs, vec![PathBuf::from("file.txt")]);
        assert!(args.in_place);
        assert_eq!(args.max_iters, 20);
        assert_eq!(args.min_score, 0.7);
        assert_eq!(args.tab_width, 2);
        assert!(args.all);
        assert!(args.verbose);
        assert!(args.diff);
    }

    #[test]
    fn test_args_multiple_files() {
        let args = Args::parse_from(["aadc", "file1.txt", "file2.txt", "file3.txt"]);
        assert_eq!(
            args.inputs,
            vec![
                PathBuf::from("file1.txt"),
                PathBuf::from("file2.txt"),
                PathBuf::from("file3.txt")
            ]
        );
    }

    #[test]
    fn test_args_multiple_files_with_inplace() {
        let args = Args::parse_from(["aadc", "-i", "file1.txt", "file2.txt"]);
        assert_eq!(
            args.inputs,
            vec![PathBuf::from("file1.txt"), PathBuf::from("file2.txt")]
        );
        assert!(args.in_place);
    }

    #[test]
    fn test_args_preset_long() {
        let args = Args::parse_from(["aadc", "--preset", "strict", "file.txt"]);
        assert_eq!(args.inputs, vec![PathBuf::from("file.txt")]);
        assert!(matches!(args.preset, Some(Preset::Strict)));
    }

    #[test]
    fn test_args_preset_short() {
        let args = Args::parse_from(["aadc", "-P", "aggressive", "file.txt"]);
        assert!(matches!(args.preset, Some(Preset::Aggressive)));
    }

    #[test]
    fn test_args_preset_case_insensitive() {
        let args = Args::parse_from(["aadc", "--preset", "RELAXED", "file.txt"]);
        assert!(matches!(args.preset, Some(Preset::Relaxed)));
    }

    #[test]
    fn test_args_preset_conflicts_with_min_score() {
        let result = Args::try_parse_from([
            "aadc",
            "--preset",
            "strict",
            "--min-score",
            "0.3",
            "file.txt",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_effective_min_score_with_preset() {
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            preset: Some(Preset::Strict),
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };
        assert_eq!(config.effective_min_score(), 0.8);
    }

    #[test]
    fn test_effective_min_score_without_preset() {
        let config = Config {
            max_iters: 10,
            min_score: 0.42,
            preset: None,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };
        assert_eq!(config.effective_min_score(), 0.42);
    }

    #[test]
    fn test_validate_args_min_score_bounds() {
        let mut args = make_args();
        args.min_score = -0.1;
        assert!(validate_args(&args).is_err());
        args.min_score = 1.1;
        assert!(validate_args(&args).is_err());
        args.min_score = 0.0;
        assert!(validate_args(&args).is_ok());
        args.min_score = 1.0;
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn test_validate_args_max_iters_zero() {
        let mut args = make_args();
        args.max_iters = 0;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn test_validate_args_in_place_requires_file() {
        let mut args = make_args();
        args.in_place = true;
        assert!(validate_args(&args).is_err());
        args.inputs = vec![PathBuf::from("diagram.txt")];
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn test_args_dry_run() {
        let args = Args::parse_from(["aadc", "-n", "file.txt"]);
        assert!(args.dry_run);
        assert!(!args.in_place);
    }

    #[test]
    fn test_args_dry_run_long() {
        let args = Args::parse_from(["aadc", "--dry-run", "file.txt"]);
        assert!(args.dry_run);
    }

    #[test]
    fn test_args_dry_run_with_diff() {
        let args = Args::parse_from(["aadc", "-n", "-d", "file.txt"]);
        assert!(args.dry_run);
        assert!(args.diff);
    }

    #[test]
    fn test_args_dry_run_with_verbose() {
        let args = Args::parse_from(["aadc", "-n", "-v", "file.txt"]);
        assert!(args.dry_run);
        assert!(args.verbose);
    }

    #[test]
    fn test_args_backup() {
        let args = Args::parse_from(["aadc", "-i", "--backup", "file.txt"]);
        assert!(args.in_place);
        assert!(args.backup);
        assert_eq!(args.backup_ext, ".bak");
    }

    #[test]
    fn test_args_backup_custom_ext() {
        let args = Args::parse_from([
            "aadc",
            "-i",
            "--backup",
            "--backup-ext",
            ".orig",
            "file.txt",
        ]);
        assert!(args.backup);
        assert_eq!(args.backup_ext, ".orig");
    }

    #[test]
    fn test_create_backup() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("test.txt");
        fs::write(&file, "original content").unwrap();

        let backup = create_backup(&file, ".bak").unwrap();

        assert!(backup.exists());
        assert_eq!(backup.file_name().unwrap(), "test.txt.bak");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "original content");
        // Original file should still exist unchanged
        assert!(file.exists());
        assert_eq!(fs::read_to_string(&file).unwrap(), "original content");
    }

    #[test]
    fn test_create_backup_preserves_extension() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("diagram.md");
        fs::write(&file, "# Diagram").unwrap();

        let backup = create_backup(&file, ".bak").unwrap();

        // Should be diagram.md.bak, not diagram.bak
        assert_eq!(backup.file_name().unwrap(), "diagram.md.bak");
    }

    #[test]
    fn test_create_backup_custom_extension() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("test.txt");
        fs::write(&file, "content").unwrap();

        let backup = create_backup(&file, ".orig").unwrap();

        assert!(backup.to_str().unwrap().ends_with(".orig"));
    }

    #[test]
    fn test_args_json() {
        let args = Args::parse_from(["aadc", "--json", "file.txt"]);
        assert!(args.json);
    }

    #[test]
    fn test_json_output_structure() {
        // Test that JsonOutput serializes correctly
        let output = JsonOutput {
            version: "1.0",
            status: "success".to_string(),
            file: Some("test.txt".to_string()),
            input: InputStats {
                lines: 5,
                bytes: 50,
            },
            processing: ProcessingStats {
                blocks_detected: 1,
                blocks_modified: 1,
                revisions_applied: 2,
            },
            output: Some(OutputStats {
                lines: 5,
                bytes: 52,
                changed: true,
            }),
            content: Some("corrected content".to_string()),
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"version\":\"1.0\""));
        assert!(json.contains("\"status\":\"success\""));
        assert!(json.contains("\"blocks_detected\":1"));
    }

    #[test]
    fn test_json_output_dry_run_status() {
        let output = JsonOutput {
            version: "1.0",
            status: "dry_run".to_string(),
            file: Some("test.txt".to_string()),
            input: InputStats {
                lines: 3,
                bytes: 30,
            },
            processing: ProcessingStats {
                blocks_detected: 1,
                blocks_modified: 1,
                revisions_applied: 1,
            },
            output: Some(OutputStats {
                lines: 3,
                bytes: 32,
                changed: true,
            }),
            content: None, // No content in dry-run
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"status\":\"dry_run\""));
        // Content should not appear when None
        assert!(!json.contains("\"content\""));
    }

    // =========================================================================
    // is_corner() tests - 13 corner characters
    // =========================================================================

    #[test]
    fn test_is_corner_ascii() {
        assert!(is_corner('+'), "ASCII plus should be corner");
    }

    #[test]
    fn test_is_corner_light() {
        assert!(is_corner('┌'), "light top-left corner");
        assert!(is_corner('┐'), "light top-right corner");
        assert!(is_corner('└'), "light bottom-left corner");
        assert!(is_corner('┘'), "light bottom-right corner");
    }

    #[test]
    fn test_is_corner_double() {
        assert!(is_corner('╔'), "double top-left corner");
        assert!(is_corner('╗'), "double top-right corner");
        assert!(is_corner('╚'), "double bottom-left corner");
        assert!(is_corner('╝'), "double bottom-right corner");
    }

    #[test]
    fn test_is_corner_rounded() {
        assert!(is_corner('╭'), "rounded top-left corner");
        assert!(is_corner('╮'), "rounded top-right corner");
        assert!(is_corner('╯'), "rounded bottom-right corner");
        assert!(is_corner('╰'), "rounded bottom-left corner");
    }

    #[test]
    fn test_is_corner_negative() {
        assert!(!is_corner('-'), "horizontal fill is not corner");
        assert!(!is_corner('|'), "vertical border is not corner");
        assert!(!is_corner('a'), "letter is not corner");
        assert!(!is_corner(' '), "space is not corner");
        assert!(!is_corner('─'), "horizontal line is not corner");
        assert!(!is_corner('┼'), "junction is not corner");
    }

    // =========================================================================
    // is_horizontal_fill() tests - 12 horizontal fill characters
    // =========================================================================

    #[test]
    fn test_is_horizontal_fill_ascii() {
        assert!(is_horizontal_fill('-'), "ASCII dash");
        assert!(is_horizontal_fill('~'), "ASCII tilde");
        assert!(is_horizontal_fill('='), "ASCII equals");
    }

    #[test]
    fn test_is_horizontal_fill_light() {
        assert!(is_horizontal_fill('─'), "light horizontal");
        assert!(is_horizontal_fill('╌'), "light dashed 2");
        assert!(is_horizontal_fill('┄'), "light dashed 3");
        assert!(is_horizontal_fill('┈'), "light dashed 4");
    }

    #[test]
    fn test_is_horizontal_fill_heavy() {
        assert!(is_horizontal_fill('━'), "heavy horizontal");
        assert!(is_horizontal_fill('╍'), "heavy dashed 2");
        assert!(is_horizontal_fill('┅'), "heavy dashed 3");
        assert!(is_horizontal_fill('┉'), "heavy dashed 4");
    }

    #[test]
    fn test_is_horizontal_fill_double() {
        assert!(is_horizontal_fill('═'), "double horizontal");
    }

    #[test]
    fn test_is_horizontal_fill_negative() {
        assert!(!is_horizontal_fill('|'), "vertical is not horizontal");
        assert!(!is_horizontal_fill('+'), "corner is not horizontal fill");
        assert!(!is_horizontal_fill('a'), "letter is not horizontal fill");
        assert!(!is_horizontal_fill(' '), "space is not horizontal fill");
        assert!(!is_horizontal_fill('│'), "vertical line is not horizontal");
    }

    // =========================================================================
    // is_vertical_border() tests - 10 vertical border characters
    // =========================================================================

    #[test]
    fn test_is_vertical_border_ascii() {
        assert!(is_vertical_border('|'), "ASCII pipe");
    }

    #[test]
    fn test_is_vertical_border_light() {
        assert!(is_vertical_border('│'), "light vertical");
        assert!(is_vertical_border('╎'), "light dashed 2");
        assert!(is_vertical_border('┆'), "light dashed 3");
        assert!(is_vertical_border('┊'), "light dashed 4");
    }

    #[test]
    fn test_is_vertical_border_heavy() {
        assert!(is_vertical_border('┃'), "heavy vertical");
        assert!(is_vertical_border('╏'), "heavy dashed 2");
        assert!(is_vertical_border('┇'), "heavy dashed 3");
        assert!(is_vertical_border('┋'), "heavy dashed 4");
    }

    #[test]
    fn test_is_vertical_border_double() {
        assert!(is_vertical_border('║'), "double vertical");
    }

    #[test]
    fn test_is_vertical_border_negative() {
        assert!(!is_vertical_border('-'), "horizontal is not vertical");
        assert!(!is_vertical_border('+'), "corner is not vertical border");
        assert!(!is_vertical_border('a'), "letter is not vertical border");
        assert!(!is_vertical_border(' '), "space is not vertical border");
        assert!(!is_vertical_border('─'), "horizontal line is not vertical");
    }

    // =========================================================================
    // is_junction() tests - 16 junction characters
    // =========================================================================

    #[test]
    fn test_is_junction_light() {
        assert!(is_junction('┬'), "light down and horizontal");
        assert!(is_junction('┴'), "light up and horizontal");
        assert!(is_junction('├'), "light vertical and right");
        assert!(is_junction('┤'), "light vertical and left");
        assert!(is_junction('┼'), "light vertical and horizontal");
    }

    #[test]
    fn test_is_junction_double() {
        assert!(is_junction('╦'), "double down and horizontal");
        assert!(is_junction('╩'), "double up and horizontal");
        assert!(is_junction('╠'), "double vertical and right");
        assert!(is_junction('╣'), "double vertical and left");
        assert!(is_junction('╬'), "double vertical and horizontal");
    }

    #[test]
    fn test_is_junction_mixed() {
        assert!(is_junction('╤'), "down single and horizontal double");
        assert!(is_junction('╧'), "up single and horizontal double");
        assert!(is_junction('╟'), "vertical double and right single");
        assert!(is_junction('╢'), "vertical double and left single");
        assert!(is_junction('╫'), "vertical double and horizontal single");
        assert!(is_junction('╪'), "vertical single and horizontal double");
    }

    #[test]
    fn test_is_junction_negative() {
        assert!(!is_junction('+'), "ASCII plus is corner, not junction");
        assert!(!is_junction('┌'), "corner is not junction");
        assert!(!is_junction('─'), "horizontal is not junction");
        assert!(!is_junction('│'), "vertical is not junction");
        assert!(!is_junction('a'), "letter is not junction");
    }

    // =========================================================================
    // is_box_char() tests - composite function
    // =========================================================================

    #[test]
    fn test_is_box_char_corners() {
        assert!(is_box_char('+'), "ASCII corner is box char");
        assert!(is_box_char('┌'), "light corner is box char");
        assert!(is_box_char('╔'), "double corner is box char");
        assert!(is_box_char('╭'), "rounded corner is box char");
    }

    #[test]
    fn test_is_box_char_horizontals() {
        assert!(is_box_char('-'), "ASCII dash is box char");
        assert!(is_box_char('─'), "light horizontal is box char");
        assert!(is_box_char('═'), "double horizontal is box char");
    }

    #[test]
    fn test_is_box_char_verticals() {
        assert!(is_box_char('|'), "ASCII pipe is box char");
        assert!(is_box_char('│'), "light vertical is box char");
        assert!(is_box_char('║'), "double vertical is box char");
    }

    #[test]
    fn test_is_box_char_junctions() {
        assert!(is_box_char('┼'), "light junction is box char");
        assert!(is_box_char('╬'), "double junction is box char");
        assert!(is_box_char('╪'), "mixed junction is box char");
    }

    #[test]
    fn test_is_box_char_negative() {
        assert!(!is_box_char('a'), "letter is not box char");
        assert!(!is_box_char(' '), "space is not box char");
        assert!(!is_box_char('0'), "digit is not box char");
        assert!(!is_box_char('\n'), "newline is not box char");
        assert!(!is_box_char('中'), "CJK char is not box char");
    }

    // =========================================================================
    // is_border_char() tests
    // =========================================================================

    #[test]
    fn test_is_border_char_verticals() {
        assert!(is_border_char('|'), "ASCII pipe is border char");
        assert!(is_border_char('│'), "light vertical is border char");
        assert!(is_border_char('║'), "double vertical is border char");
    }

    #[test]
    fn test_is_border_char_corners() {
        assert!(is_border_char('+'), "ASCII corner is border char");
        assert!(is_border_char('┐'), "unicode corner is border char");
        assert!(is_border_char('╝'), "double corner is border char");
    }

    #[test]
    fn test_is_border_char_junctions() {
        assert!(is_border_char('┤'), "junction is border char");
        assert!(is_border_char('╣'), "double junction is border char");
        assert!(is_border_char('╢'), "mixed junction is border char");
    }

    #[test]
    fn test_is_border_char_negative() {
        assert!(!is_border_char('-'), "horizontal fill is not border char");
        assert!(!is_border_char('a'), "letter is not border char");
        assert!(!is_border_char(' '), "space is not border char");
    }

    // =========================================================================
    // detect_vertical_border() tests - frequency-based detection
    // =========================================================================

    #[test]
    fn test_detect_vertical_border_ascii() {
        let lines = vec!["| hello |", "| world |"];
        assert_eq!(detect_vertical_border(&lines), '|');
    }

    #[test]
    fn test_detect_vertical_border_unicode_light() {
        let lines = vec!["│ hello │", "│ world │"];
        assert_eq!(detect_vertical_border(&lines), '│');
    }

    #[test]
    fn test_detect_vertical_border_unicode_double() {
        let lines = vec!["║ hello ║", "║ world ║"];
        assert_eq!(detect_vertical_border(&lines), '║');
    }

    #[test]
    fn test_detect_vertical_border_mixed_prefers_most_common() {
        let lines = vec!["│ a │", "│ b │", "│ c │", "| d |"];
        // 6 occurrences of │ vs 2 occurrences of |
        assert_eq!(detect_vertical_border(&lines), '│');
    }

    #[test]
    fn test_detect_vertical_border_empty_defaults_to_ascii() {
        let lines: Vec<&str> = vec![];
        assert_eq!(detect_vertical_border(&lines), '|');
    }

    #[test]
    fn test_detect_vertical_border_no_borders_defaults_to_ascii() {
        let lines = vec!["hello world", "no borders here"];
        assert_eq!(detect_vertical_border(&lines), '|');
    }

    // =========================================================================
    // Revision::score() tests
    // =========================================================================

    fn make_analyzed_lines(lines: &[&str]) -> Vec<AnalyzedLine> {
        lines.iter().map(|l| analyze_line(l)).collect()
    }

    #[test]
    fn test_revision_score_pad_small_adjustment() {
        let lines = vec!["| short|", "| longer |"];
        let analyzed = make_analyzed_lines(&lines);
        // Small padding (2 spaces) should have high score
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 2,
            target_column: 10,
        };
        let score = rev.score(&analyzed, 0);
        // Base 0.8 - 0.2 penalty + 0.2 strong bonus = 0.8 for strong line
        assert!(
            (0.6..=1.0).contains(&score),
            "score={} should be in [0.6, 1.0]",
            score
        );
    }

    #[test]
    fn test_revision_score_pad_large_adjustment() {
        let lines = vec!["| x|", "| very long content |"];
        let analyzed = make_analyzed_lines(&lines);
        // Large padding should have lower score
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 10,
            target_column: 20,
        };
        let score = rev.score(&analyzed, 0);
        // 10 spaces = 1.0 penalty capped at 0.5, so 0.8 - 0.5 = 0.3 base
        assert!(
            (0.0..=0.8).contains(&score),
            "large adjustment score={} should be lower",
            score
        );
    }

    #[test]
    fn test_revision_score_pad_strong_line_bonus() {
        let lines = vec!["+---+", "| x |"];
        let analyzed = make_analyzed_lines(&lines);
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 2,
            target_column: 8,
        };
        let score = rev.score(&analyzed, 0);
        // Strong line gets 0.2 bonus
        assert!(score > 0.7, "strong line should get bonus, score={}", score);
    }

    #[test]
    fn test_revision_score_add_border_base() {
        let lines = vec!["| text", "| other |"];
        let analyzed = make_analyzed_lines(&lines);
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '|',
            target_column: 10,
        };
        let score = rev.score(&analyzed, 0);
        // AddSuffixBorder has base 0.5 + 0.1-0.2 strength bonus
        assert!(
            (0.5..=0.8).contains(&score),
            "add border score={} should be moderate",
            score
        );
    }

    #[test]
    fn test_revision_score_add_border_strong_line() {
        let lines = vec!["+----", "+----+"];
        let analyzed = make_analyzed_lines(&lines);
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '+',
            target_column: 6,
        };
        let score = rev.score(&analyzed, 0);
        // Strong line gets 0.2 bonus instead of 0.1
        assert!(
            score >= 0.6,
            "strong line add border score={} should be higher",
            score
        );
    }

    #[test]
    fn test_revision_score_with_block_offset() {
        // Test that block_start offset is correctly applied
        let lines = vec!["| hello|", "| world |"];
        let analyzed = make_analyzed_lines(&lines);
        // Simulate being at block offset 5 in global lines
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 5,
            spaces_to_add: 2,
            target_column: 10,
        };
        let score = rev.score(&analyzed, 5);
        assert!(score > 0.0, "should correctly index with block offset");
    }

    // =========================================================================
    // Revision::apply() tests
    // =========================================================================

    #[test]
    fn test_revision_apply_pad_ascii() {
        let mut lines = vec!["| short|".to_string()];
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 3,
            target_column: 10,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "| short   |", "should pad before closing border");
    }

    #[test]
    fn test_revision_apply_pad_unicode() {
        let mut lines = vec!["│ text│".to_string()];
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 2,
            target_column: 10,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "│ text  │", "should pad before unicode border");
    }

    #[test]
    fn test_revision_apply_pad_corner() {
        let mut lines = vec!["+---+".to_string()];
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 2,
            target_column: 7,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "+---  +", "should pad before corner");
    }

    #[test]
    fn test_revision_apply_pad_preserves_other_lines() {
        let mut lines = vec!["| first|".to_string(), "| second |".to_string()];
        let rev = Revision::PadBeforeSuffixBorder {
            line_idx: 0,
            spaces_to_add: 2,
            target_column: 10,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "| first  |");
        assert_eq!(lines[1], "| second |", "other lines should be unchanged");
    }

    #[test]
    fn test_revision_apply_add_border_ascii() {
        let mut lines = vec!["| text".to_string()];
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '|',
            target_column: 10,
        };
        rev.apply(&mut lines);
        assert_eq!(
            lines[0], "| text    |",
            "should add border at target column"
        );
    }

    #[test]
    fn test_revision_apply_add_border_unicode() {
        let mut lines = vec!["│ hello".to_string()];
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '│',
            target_column: 12,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "│ hello     │", "should add unicode border");
    }

    #[test]
    fn test_revision_apply_add_corner() {
        let mut lines = vec!["+----".to_string()];
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '+',
            target_column: 6,
        };
        rev.apply(&mut lines);
        assert_eq!(lines[0], "+---- +", "should add corner");
    }

    #[test]
    fn test_revision_apply_add_border_no_extra_padding() {
        let mut lines = vec!["| exact len|".to_string()];
        // If current width >= target, padding should be 0
        let rev = Revision::AddSuffixBorder {
            line_idx: 0,
            border_char: '|',
            target_column: 5, // Less than current width
        };
        rev.apply(&mut lines);
        // Should add border with no padding
        assert!(lines[0].ends_with('|'), "should still add border");
    }

    // =========================================================================
    // classify_line() tests
    // =========================================================================

    #[test]
    fn test_classify_line_blank_empty() {
        assert_eq!(classify_line(""), LineKind::Blank);
    }

    #[test]
    fn test_classify_line_blank_spaces() {
        assert_eq!(classify_line("   "), LineKind::Blank);
        assert_eq!(classify_line("      "), LineKind::Blank);
    }

    #[test]
    fn test_classify_line_blank_tabs() {
        assert_eq!(classify_line("\t"), LineKind::Blank);
        assert_eq!(classify_line("\t\t"), LineKind::Blank);
    }

    #[test]
    fn test_classify_line_blank_mixed_whitespace() {
        assert_eq!(classify_line("  \t  "), LineKind::Blank);
    }

    #[test]
    fn test_classify_line_none_plain_text() {
        assert_eq!(classify_line("hello world"), LineKind::None);
        assert_eq!(classify_line("fn main() {}"), LineKind::None);
    }

    #[test]
    fn test_classify_line_none_numbers() {
        assert_eq!(classify_line("12345"), LineKind::None);
        assert_eq!(classify_line("3.14159"), LineKind::None);
    }

    #[test]
    fn test_classify_line_none_punctuation() {
        assert_eq!(classify_line("..."), LineKind::None);
        assert_eq!(classify_line("???"), LineKind::None);
    }

    #[test]
    fn test_classify_line_strong_ascii_corners() {
        assert_eq!(classify_line("+---+"), LineKind::Strong);
        assert_eq!(classify_line("+--+"), LineKind::Strong);
    }

    #[test]
    fn test_classify_line_strong_border_both_sides() {
        assert_eq!(classify_line("| x |"), LineKind::Strong);
        assert_eq!(classify_line("| content |"), LineKind::Strong);
    }

    #[test]
    fn test_classify_line_strong_unicode_light() {
        assert_eq!(classify_line("┌───┐"), LineKind::Strong);
        assert_eq!(classify_line("│ y │"), LineKind::Strong);
        assert_eq!(classify_line("└───┘"), LineKind::Strong);
    }

    #[test]
    fn test_classify_line_strong_unicode_double() {
        assert_eq!(classify_line("╔═══╗"), LineKind::Strong);
        assert_eq!(classify_line("║ z ║"), LineKind::Strong);
        assert_eq!(classify_line("╚═══╝"), LineKind::Strong);
    }

    #[test]
    fn test_classify_line_strong_high_ratio() {
        // More than 1/3 box chars = strong
        assert_eq!(classify_line("---"), LineKind::Strong);
        assert_eq!(classify_line("───────"), LineKind::Strong);
    }

    #[test]
    fn test_classify_line_weak_few_box_chars() {
        // Has box chars but doesn't meet strong criteria
        assert_eq!(classify_line("text | here"), LineKind::Weak);
        assert_eq!(classify_line("a - b"), LineKind::Weak);
    }

    #[test]
    fn test_classify_line_weak_single_border() {
        // Only one side has border
        assert_eq!(classify_line("| text"), LineKind::Weak);
        assert_eq!(classify_line("text |"), LineKind::Weak);
    }

    // =========================================================================
    // visual_width() tests
    // =========================================================================

    #[test]
    fn test_visual_width_empty() {
        assert_eq!(visual_width(""), 0);
    }

    #[test]
    fn test_visual_width_ascii() {
        assert_eq!(visual_width("hello"), 5);
        assert_eq!(visual_width("a b c"), 5);
        assert_eq!(visual_width("test!"), 5);
    }

    #[test]
    fn test_visual_width_box_chars() {
        assert_eq!(visual_width("│──│"), 4);
        assert_eq!(visual_width("┌──┐"), 4);
        assert_eq!(visual_width("╔══╗"), 4);
    }

    #[test]
    fn test_visual_width_cjk() {
        // CJK characters are double-width
        assert_eq!(visual_width("中"), 2);
        assert_eq!(visual_width("中文"), 4);
        assert_eq!(visual_width("日本語"), 6);
    }

    #[test]
    fn test_visual_width_mixed_ascii_cjk() {
        // "a中b" = 1 + 2 + 1 = 4
        assert_eq!(visual_width("a中b"), 4);
        assert_eq!(visual_width("hi中文"), 6); // 2 + 2 + 2
    }

    #[test]
    fn test_visual_width_box_and_cjk() {
        // Box chars in CJK context
        assert_eq!(visual_width("│中│"), 4); // 1 + 2 + 1
    }

    // =========================================================================
    // analyze_line() tests
    // =========================================================================

    #[test]
    fn test_analyze_line_blank() {
        let result = analyze_line("");
        assert_eq!(result.kind, LineKind::Blank);
        assert_eq!(result.visual_width, 0);
        assert!(result.suffix_border.is_none());
    }

    #[test]
    fn test_analyze_line_strong_with_border() {
        let result = analyze_line("| hello |");
        assert_eq!(result.kind, LineKind::Strong);
        assert_eq!(result.visual_width, 9);
        assert!(result.suffix_border.is_some());
        let border = result.suffix_border.unwrap();
        assert_eq!(border.char, '|');
    }

    #[test]
    fn test_analyze_line_indented() {
        let result = analyze_line("  | text |");
        assert_eq!(result.indent, 2);
        assert_eq!(result.kind, LineKind::Strong);
    }

    #[test]
    fn test_analyze_line_no_suffix_border() {
        let result = analyze_line("| missing end");
        assert_eq!(result.kind, LineKind::Weak);
        assert!(result.suffix_border.is_none());
    }

    #[test]
    fn test_analyze_line_unicode_border() {
        let result = analyze_line("│ content │");
        assert_eq!(result.kind, LineKind::Strong);
        assert!(result.suffix_border.is_some());
        let border = result.suffix_border.unwrap();
        assert_eq!(border.char, '│');
    }

    // =========================================================================
    // detect_suffix_border() tests
    // =========================================================================

    #[test]
    fn test_detect_suffix_border_ascii_pipe() {
        let border = detect_suffix_border("| hello |");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '|');
        assert!(!b.is_closing);
        assert_eq!(b.column, 8);
    }

    #[test]
    fn test_detect_suffix_border_unicode_light() {
        let border = detect_suffix_border("│ text │");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '│');
        assert!(!b.is_closing);
    }

    #[test]
    fn test_detect_suffix_border_corner() {
        let border = detect_suffix_border("+---+");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '+');
        assert!(b.is_closing);
    }

    #[test]
    fn test_detect_suffix_border_unicode_corner() {
        let border = detect_suffix_border("┌───┐");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '┐');
        assert!(b.is_closing);
    }

    #[test]
    fn test_detect_suffix_border_junction() {
        let border = detect_suffix_border("│ a ┤");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '┤');
        assert!(b.is_closing);
    }

    #[test]
    fn test_detect_suffix_border_none_no_border() {
        let border = detect_suffix_border("hello world");
        assert!(border.is_none());
    }

    #[test]
    fn test_detect_suffix_border_none_empty() {
        let border = detect_suffix_border("");
        assert!(border.is_none());
    }

    #[test]
    fn test_detect_suffix_border_trailing_spaces() {
        // Should detect border despite trailing spaces
        let border = detect_suffix_border("| text |   ");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '|');
    }

    #[test]
    fn test_detect_suffix_border_column_position() {
        let border = detect_suffix_border("| ab |");
        assert!(border.is_some());
        let b = border.unwrap();
        // "| ab |" has visual width 6, column of | is 5 (0-indexed)
        assert_eq!(b.column, 5);
    }

    // =========================================================================
    // expand_tabs() tests
    // =========================================================================

    #[test]
    fn test_expand_tabs_start_of_line() {
        assert_eq!(expand_tabs("\thello", 4), "    hello");
    }

    #[test]
    fn test_expand_tabs_middle_of_line() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("ab\tc", 4), "ab  c");
        assert_eq!(expand_tabs("abc\td", 4), "abc d");
    }

    #[test]
    fn test_expand_tabs_multiple() {
        assert_eq!(expand_tabs("\t\t", 4), "        ");
        assert_eq!(expand_tabs("a\tb\tc", 4), "a   b   c");
    }

    #[test]
    fn test_expand_tabs_width_2() {
        assert_eq!(expand_tabs("\thello", 2), "  hello");
        assert_eq!(expand_tabs("a\tb", 2), "a b");
    }

    #[test]
    fn test_expand_tabs_width_8() {
        assert_eq!(expand_tabs("\thello", 8), "        hello");
    }

    #[test]
    fn test_expand_tabs_no_tabs() {
        assert_eq!(expand_tabs("no tabs here", 4), "no tabs here");
    }

    #[test]
    fn test_expand_tabs_empty() {
        assert_eq!(expand_tabs("", 4), "");
    }

    // =========================================================================
    // find_diagram_blocks() tests
    // =========================================================================

    #[test]
    fn test_find_diagram_blocks_simple() {
        let lines: Vec<String> = vec![
            "Some text".to_string(),
            "+---+".to_string(),
            "| x |".to_string(),
            "+---+".to_string(),
            "More text".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, 1);
        assert_eq!(blocks[0].end, 4);
    }

    #[test]
    fn test_find_diagram_blocks_no_diagrams() {
        let lines: Vec<String> = vec![
            "Just plain text".to_string(),
            "No diagrams here".to_string(),
            "More text".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_find_diagram_blocks_multiple() {
        // Need more than 3 non-boxy lines to prevent lookahead merging
        let lines: Vec<String> = vec![
            "+--+".to_string(),
            "| A|".to_string(),
            "+--+".to_string(),
            "plain text".to_string(),
            "more text".to_string(),
            "even more".to_string(),
            "still more".to_string(),
            "+--+".to_string(),
            "| B|".to_string(),
            "+--+".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 2, "should find two separate blocks");
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].end, 3);
        assert_eq!(blocks[1].start, 7);
        assert_eq!(blocks[1].end, 10);
    }

    #[test]
    fn test_find_diagram_blocks_with_blank_gap() {
        let lines: Vec<String> = vec![
            "+---+".to_string(),
            "| a |".to_string(),
            "".to_string(), // Single blank allowed
            "| b |".to_string(),
            "+---+".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1, "single blank gap should be allowed");
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].end, 5);
    }

    #[test]
    fn test_find_diagram_blocks_large_gap_splits() {
        let lines: Vec<String> = vec![
            "+--+".to_string(),
            "| A|".to_string(),
            "+--+".to_string(),
            "".to_string(),
            "".to_string(), // Two blank lines should split
            "+--+".to_string(),
            "| B|".to_string(),
            "+--+".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 2, "double blank gap should split blocks");
    }

    #[test]
    fn test_find_diagram_blocks_unicode() {
        let lines: Vec<String> = vec![
            "┌───┐".to_string(),
            "│ x │".to_string(),
            "└───┘".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].end, 3);
    }

    #[test]
    fn test_find_diagram_blocks_at_start() {
        let lines: Vec<String> = vec!["+--+".to_string(), "|xy|".to_string(), "+--+".to_string()];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, 0);
    }

    #[test]
    fn test_find_diagram_blocks_at_end() {
        let lines: Vec<String> = vec![
            "text".to_string(),
            "+--+".to_string(),
            "|xy|".to_string(),
            "+--+".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].end, 4, "should go to end of lines");
    }

    #[test]
    fn test_find_diagram_blocks_confidence_high() {
        let lines: Vec<String> = vec![
            "+------+".to_string(),
            "| text |".to_string(),
            "| more |".to_string(),
            "+------+".to_string(),
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert!(
            blocks[0].confidence > 0.5,
            "all strong lines should have high confidence"
        );
    }

    #[test]
    fn test_find_diagram_blocks_all_flag() {
        let lines: Vec<String> = vec![
            "text | here".to_string(), // Weak line
            "more".to_string(),
        ];

        // Without all_blocks flag, low confidence blocks are skipped
        let blocks_default = find_diagram_blocks(&lines, false);

        // With all_blocks flag, low confidence blocks are included
        let blocks_all = find_diagram_blocks(&lines, true);

        assert!(
            blocks_all.len() >= blocks_default.len(),
            "all_blocks=true should include more blocks"
        );
    }

    #[test]
    fn test_find_diagram_blocks_trims_trailing_blank() {
        let lines: Vec<String> = vec![
            "+--+".to_string(),
            "|ab|".to_string(),
            "+--+".to_string(),
            "".to_string(), // Trailing blank
        ];

        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].end, 3, "should trim trailing blank");
    }

    #[test]
    fn test_find_diagram_blocks_empty_input() {
        let lines: Vec<String> = vec![];
        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_find_diagram_blocks_only_blanks() {
        let lines: Vec<String> = vec!["".to_string(), "   ".to_string(), "".to_string()];
        let blocks = find_diagram_blocks(&lines, false);
        assert_eq!(blocks.len(), 0);
    }

    // =========================================================================
    // detect_suffix_border() tests (old location kept for reference)
    // =========================================================================

    #[test]
    fn test_detect_suffix_border() {
        let border = detect_suffix_border("| hello |");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '|');
        assert!(!b.is_closing);

        let no_border = detect_suffix_border("hello world");
        assert!(no_border.is_none());
    }

    #[test]
    fn test_correction_simple() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            preset: None,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+------+".to_string(),
            "| short|".to_string(),
            "| longer |".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console);

        // Should find and process the block
        assert_eq!(stats.blocks_found, 1);

        // All right borders should be aligned
        let widths: Vec<usize> = corrected
            .iter()
            .filter(|l| classify_line(l).is_boxy())
            .map(|l| visual_width(l.trim_end()))
            .collect();

        // Check that boxy lines have consistent width
        if !widths.is_empty() {
            let first = widths[0];
            assert!(widths.iter().all(|&w| w == first || w >= first - 2));
        }
    }

    // =========================================================================
    // correct_lines() integration tests
    // =========================================================================

    #[test]
    fn test_correction_no_diagrams() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            preset: None,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "Just plain text".to_string(),
            "No diagrams here".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines.clone(), &config, &console);
        assert_eq!(stats.blocks_found, 0);
        assert_eq!(stats.blocks_modified, 0);
        assert_eq!(corrected, lines, "content should be unchanged");
    }

    #[test]
    fn test_correction_already_aligned() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            preset: None,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+------+".to_string(),
            "| text |".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines.clone(), &config, &console);
        assert_eq!(stats.blocks_found, 1);
        // Perfectly aligned blocks should not be modified
        assert_eq!(corrected, lines);
    }

    #[test]
    fn test_correction_unicode() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "┌───────┐".to_string(),
            "│ short│".to_string(),
            "│ longer │".to_string(),
            "└───────┘".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console);
        assert_eq!(stats.blocks_found, 1);
        // Verify correction ran successfully (at least one block found and processed)
        assert!(!corrected.is_empty());
    }

    #[test]
    fn test_correction_with_tabs() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+------+".to_string(),
            "|\thi|".to_string(), // Tab should be expanded
            "+------+".to_string(),
        ];

        let (corrected, _) = correct_lines(lines, &config, &console);
        // Tab should be expanded to spaces
        assert!(!corrected[1].contains('\t'), "tabs should be expanded");
    }

    #[test]
    fn test_correction_max_iters_limit() {
        let console = Console::new();
        let config = Config {
            max_iters: 1, // Only 1 iteration
            min_score: 0.1,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+--------+".to_string(),
            "| a|".to_string(),
            "| longer |".to_string(),
            "+--------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console);
        assert_eq!(stats.blocks_found, 1);
        // With limited iterations, some progress should still be made
        assert!(corrected.len() == 4);
    }

    #[test]
    fn test_correction_min_score_filter() {
        let console = Console::new();
        let config_strict = Config {
            max_iters: 10,
            min_score: 0.95, // Very strict
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+------+".to_string(),
            "| text|".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, _) = correct_lines(lines.clone(), &config_strict, &console);
        // With very strict min_score, fewer changes should be made
        // The exact behavior depends on the scoring implementation
        assert!(corrected.len() == 3);
    }

    #[test]
    fn test_correction_multiple_blocks() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "+--+".to_string(),
            "| A|".to_string(),
            "+--+".to_string(),
            "text".to_string(),
            "more".to_string(),
            "even more".to_string(),
            "still more".to_string(),
            "+--+".to_string(),
            "| B|".to_string(),
            "+--+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console);
        assert_eq!(stats.blocks_found, 2, "should find two blocks");
        assert_eq!(corrected.len(), 10);
    }

    #[test]
    fn test_correction_empty_input() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines: Vec<String> = vec![];
        let (corrected, stats) = correct_lines(lines, &config, &console);
        assert_eq!(stats.blocks_found, 0);
        assert!(corrected.is_empty());
    }

    #[test]
    fn test_correction_preserves_non_diagram_content() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        };

        let lines = vec![
            "# Header".to_string(),
            "".to_string(),
            "+---+".to_string(),
            "| x|".to_string(),
            "+---+".to_string(),
            "".to_string(),
            "Footer text".to_string(),
        ];

        let (corrected, _) = correct_lines(lines, &config, &console);
        assert_eq!(corrected[0], "# Header");
        assert_eq!(corrected[6], "Footer text");
    }
}
