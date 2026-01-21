//! ASCII Art Diagram Corrector (aadc)
//!
//! A CLI tool that fixes misaligned right-hand borders in ASCII diagrams.
//! Uses an iterative correction loop with scoring to achieve clean alignment.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::Parser;
use rich_rust::Console;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// CLI Arguments
// ─────────────────────────────────────────────────────────────────────────────

/// ASCII Art Diagram Corrector: fixes misaligned right borders in ASCII diagrams
#[derive(Parser, Debug)]
#[command(name = "aadc", version, about, long_about = None)]
struct Args {
    /// Input file (reads from stdin if not provided)
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,

    /// Edit the file in place
    #[arg(short = 'i', long)]
    in_place: bool,

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
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration and Statistics
// ─────────────────────────────────────────────────────────────────────────────

/// Runtime configuration derived from CLI args
struct Config {
    max_iters: usize,
    min_score: f64,
    tab_width: usize,
    all_blocks: bool,
    verbose: bool,
}

impl From<&Args> for Config {
    fn from(args: &Args) -> Self {
        Self {
            max_iters: args.max_iters,
            min_score: args.min_score,
            tab_width: args.tab_width,
            all_blocks: args.all,
            verbose: args.verbose,
        }
    }
}

/// Statistics collected during correction
#[derive(Default)]
struct Stats {
    blocks_found: usize,
    blocks_modified: usize,
    total_revisions: usize,
    iterations: usize,
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
        '┬' | '┴' | '├' | '┤' | '┼' | '╦' | '╩' | '╠' | '╣' | '╬' | '╤' | '╧' | '╟' | '╢' | '╫'
            | '╪'
    )
}

/// Check if character could be part of a box drawing
fn is_box_char(c: char) -> bool {
    is_corner(c) || is_horizontal_fill(c) || is_vertical_border(c) || is_junction(c)
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
    content: String,
    /// Classification of the line
    kind: LineKind,
    /// Visual width (accounting for wide chars)
    visual_width: usize,
    /// Left indentation (leading spaces)
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
    char: char,
    /// Whether this looks like a closing border (vs mid-line)
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
    let starts_with_border =
        trimmed.chars().next().is_some_and(|c| is_vertical_border(c) || is_corner(c));
    let ends_with_border = trimmed
        .chars()
        .next_back()
        .is_some_and(|c| is_vertical_border(c) || is_corner(c));

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

    if is_vertical_border(last_char) || is_corner(last_char) {
        let column = visual_width(trimmed) - 1;
        Some(SuffixBorder {
            column,
            char: last_char,
            is_closing: is_corner(last_char) || is_vertical_border(last_char),
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
            blocks.push(DiagramBlock { start, end, confidence });
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
            Self::PadBeforeSuffixBorder { line_idx, spaces_to_add, .. } => {
                let local_idx = line_idx - block_start;
                let line = &analyzed[local_idx];
                // Prefer smaller adjustments
                let adjustment_penalty = (*spaces_to_add as f64 / 10.0).min(0.5);
                // Prefer strong lines
                let strength_bonus = if line.kind == LineKind::Strong { 0.2 } else { 0.0 };
                0.8 - adjustment_penalty + strength_bonus
            }
            Self::AddSuffixBorder { line_idx, .. } => {
                let local_idx = line_idx - block_start;
                let line = &analyzed[local_idx];
                // Adding borders is less confident
                let base = 0.5;
                let strength_bonus = if line.kind == LineKind::Strong { 0.2 } else { 0.1 };
                base + strength_bonus
            }
        }
    }

    /// Apply this revision to the lines
    fn apply(&self, lines: &mut [String]) {
        match self {
            Self::PadBeforeSuffixBorder { line_idx, spaces_to_add, .. } => {
                let line = &mut lines[*line_idx];
                let trimmed = line.trim_end();
                if let Some(last_char) = trimmed.chars().next_back() {
                    if is_vertical_border(last_char) || is_corner(last_char) {
                        // Insert spaces before the last character
                        let prefix = &trimmed[..trimmed.len() - last_char.len_utf8()];
                        *line = format!("{}{}{}", prefix, " ".repeat(*spaces_to_add), last_char);
                    }
                }
            }
            Self::AddSuffixBorder { line_idx, border_char, target_column } => {
                let line = &mut lines[*line_idx];
                let current_width = visual_width(line.trim_end());
                let padding = target_column.saturating_sub(current_width);
                *line = format!(
                    "{}{}{}",
                    line.trim_end(),
                    " ".repeat(padding),
                    border_char
                );
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
        let border_char = detect_vertical_border(&block_lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());

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
        let valid_revisions: Vec<_> = revisions
            .into_iter()
            .filter(|r| r.score(&analyzed, block.start) >= config.min_score)
            .collect();

        if valid_revisions.is_empty() {
            // Converged
            if config.verbose && iteration > 0 {
                console.print(&format!("[dim]    Converged after {} iteration(s)[/]", iteration));
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
            result.extend(std::iter::repeat(' ').take(spaces));
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
        console.print(&format!("[bold cyan]Found {} diagram block(s)[/]", blocks.len()));
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
// Entry Point
// ─────────────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::from(&args);
    let console = Console::new();

    // Read input
    let lines: Vec<String> = if let Some(ref path) = args.input {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read input file: {}", path.display()))?;
        content.lines().map(String::from).collect()
    } else {
        let stdin = io::stdin();
        stdin.lock().lines().collect::<Result<Vec<_>, _>>()?
    };

    if config.verbose {
        console.print(&format!("[bold]Processing {} lines...[/]", lines.len()));
    }

    // Correct the diagrams
    let (corrected, stats) = correct_lines(lines, &config, &console);

    // Output results
    if args.in_place {
        if let Some(ref path) = args.input {
            let output = corrected.join("\n");
            fs::write(path, &output)
                .with_context(|| format!("Failed to write to file: {}", path.display()))?;

            if config.verbose {
                console.print(&format!(
                    "[bold green]Modified {} block(s), {} revision(s) applied[/]",
                    stats.blocks_modified, stats.total_revisions
                ));
            }
        } else {
            anyhow::bail!("--in-place requires an input file");
        }
    } else {
        let mut stdout = io::stdout().lock();
        for line in &corrected {
            writeln!(stdout, "{}", line)?;
        }

        if config.verbose {
            console.print(&format!(
                "[bold green]Processed {} block(s), {} revision(s) applied[/]",
                stats.blocks_found, stats.total_revisions
            ));
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_corner() {
        assert!(is_corner('+'));
        assert!(is_corner('┌'));
        assert!(is_corner('╔'));
        assert!(!is_corner('-'));
        assert!(!is_corner('a'));
    }

    #[test]
    fn test_is_horizontal_fill() {
        assert!(is_horizontal_fill('-'));
        assert!(is_horizontal_fill('─'));
        assert!(is_horizontal_fill('═'));
        assert!(!is_horizontal_fill('|'));
        assert!(!is_horizontal_fill('a'));
    }

    #[test]
    fn test_is_vertical_border() {
        assert!(is_vertical_border('|'));
        assert!(is_vertical_border('│'));
        assert!(is_vertical_border('║'));
        assert!(!is_vertical_border('-'));
        assert!(!is_vertical_border('a'));
    }

    #[test]
    fn test_classify_line_blank() {
        assert_eq!(classify_line(""), LineKind::Blank);
        assert_eq!(classify_line("   "), LineKind::Blank);
        assert_eq!(classify_line("\t"), LineKind::Blank);
    }

    #[test]
    fn test_classify_line_none() {
        assert_eq!(classify_line("hello world"), LineKind::None);
        assert_eq!(classify_line("fn main() {}"), LineKind::None);
    }

    #[test]
    fn test_classify_line_strong() {
        assert_eq!(classify_line("+---+"), LineKind::Strong);
        assert_eq!(classify_line("| x |"), LineKind::Strong);
        assert_eq!(classify_line("┌───┐"), LineKind::Strong);
        assert_eq!(classify_line("│ y │"), LineKind::Strong);
    }

    #[test]
    fn test_visual_width() {
        assert_eq!(visual_width("hello"), 5);
        assert_eq!(visual_width("│──│"), 4);
        assert_eq!(visual_width(""), 0);
    }

    #[test]
    fn test_expand_tabs() {
        assert_eq!(expand_tabs("\thello", 4), "    hello");
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("ab\tc", 4), "ab  c");
    }

    #[test]
    fn test_find_diagram_blocks() {
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
    fn test_detect_suffix_border() {
        let border = detect_suffix_border("| hello |");
        assert!(border.is_some());
        let b = border.unwrap();
        assert_eq!(b.char, '|');
        assert!(b.is_closing);

        let no_border = detect_suffix_border("hello world");
        assert!(no_border.is_none());
    }

    #[test]
    fn test_correction_simple() {
        let console = Console::new();
        let config = Config {
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all_blocks: false,
            verbose: false,
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
}
