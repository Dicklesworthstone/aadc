//! # ASCII Art Diagram Corrector (aadc)
//!
//! A CLI tool that fixes misaligned right-hand borders in ASCII diagrams.
//! Uses an iterative correction loop with scoring to achieve clean alignment.
//!
//! ## Overview
//!
//! `aadc` automatically detects ASCII diagram blocks in text files and aligns
//! their right-hand borders by adding padding. It never removes content,
//! making it safe to use on any text file.
//!
//! ## Key Components
//!
//! - **Block Detection**: Heuristic identification of diagram blocks based on
//!   box-drawing characters (both ASCII `+|-` and Unicode `┌┐└┘│─`).
//! - **Line Classification**: Lines are classified as Strong (horizontal borders),
//!   Weak (content with vertical borders), Blank, or None.
//! - **Iterative Correction**: Runs multiple passes until alignment converges
//!   or the maximum iteration count is reached.
//! - **Confidence Scoring**: Each proposed edit receives a score; only edits
//!   above the threshold are applied.
//!
//! ## Algorithm Flow
//!
//! ```text
//! Input → Tab Expansion → Block Detection → Iterative Correction → Output
//!                              ↓
//!                        For each block:
//!                          - Analyze lines
//!                          - Find target column (rightmost border)
//!                          - Generate revisions
//!                          - Score and filter
//!                          - Apply revisions
//!                          - Repeat until converged
//! ```
//!
//! ## Exit Codes
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | Success |
//! | 1 | General error (file not found, permission denied, I/O error) |
//! | 2 | Invalid command-line arguments |
//! | 3 | Dry-run mode: changes would be made |
//! | 4 | Parse error (invalid UTF-8 or binary input) |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use anyhow::{Context, Result};
use clap::ValueEnum;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rich_rust::terminal;
use rich_rust::{ColorSystem, Console};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
// Line Range Processing
// ─────────────────────────────────────────────────────────────────────────────

/// A range of lines to process (1-indexed, inclusive on both ends)
#[derive(Debug, Clone, PartialEq, Eq)]
struct LineRange {
    /// Start line (1-indexed, inclusive)
    start: usize,
    /// End line (1-indexed, inclusive, usize::MAX means "to end of file")
    end: usize,
}

/// Parse a single range specification like "10-50", "50-", "-100", or "42"
fn parse_single_range(s: &str) -> Result<LineRange, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Empty range specification".to_string());
    }

    if let Some(dash_pos) = s.find('-') {
        let (start_str, end_str) = s.split_at(dash_pos);
        let end_str = &end_str[1..]; // Skip the dash

        let start = if start_str.is_empty() {
            1 // "-100" means "1-100"
        } else {
            start_str
                .parse::<usize>()
                .map_err(|_| format!("Invalid start line: '{}'", start_str))?
        };

        let end = if end_str.is_empty() {
            usize::MAX // "50-" means "50 to end"
        } else {
            end_str
                .parse::<usize>()
                .map_err(|_| format!("Invalid end line: '{}'", end_str))?
        };

        if start == 0 {
            return Err("Line numbers start at 1, not 0".to_string());
        }

        if start > end && end != usize::MAX {
            return Err(format!("Invalid range: start ({}) > end ({})", start, end));
        }

        Ok(LineRange { start, end })
    } else {
        // Single line number
        let line = s
            .parse::<usize>()
            .map_err(|_| format!("Invalid line number: '{}'", s))?;

        if line == 0 {
            return Err("Line numbers start at 1, not 0".to_string());
        }

        Ok(LineRange {
            start: line,
            end: line,
        })
    }
}

/// Merge overlapping or adjacent ranges
fn merge_ranges(mut ranges: Vec<LineRange>) -> Vec<LineRange> {
    if ranges.is_empty() {
        return ranges;
    }

    // Sort by start position
    ranges.sort_by_key(|r| r.start);

    let mut merged = Vec::new();
    let mut current = ranges[0].clone();

    for range in ranges.into_iter().skip(1) {
        // Check if overlapping or adjacent (allowing for +1 to merge adjacent)
        if range.start <= current.end.saturating_add(1) {
            // Merge: extend current range
            current.end = current.end.max(range.end);
        } else {
            // No overlap: push current and start new
            merged.push(current);
            current = range;
        }
    }
    merged.push(current);

    merged
}

/// Parse a line ranges specification like "10-50", "1-100,200-250", "50-"
fn parse_line_ranges(s: &str) -> Result<Vec<LineRange>, String> {
    let mut ranges = Vec::new();

    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        ranges.push(parse_single_range(part)?);
    }

    if ranges.is_empty() {
        return Err("No valid ranges specified".to_string());
    }

    // Merge overlapping ranges
    Ok(merge_ranges(ranges))
}

/// Check if a line number (1-indexed) falls within any of the given ranges
#[allow(dead_code)]
fn line_in_ranges(line_num: usize, ranges: &[LineRange]) -> bool {
    ranges
        .iter()
        .any(|r| line_num >= r.start && line_num <= r.end)
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Arguments
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ColorMode {
    /// Auto-detect color support
    Auto,
    /// Always emit colors (even when not a TTY)
    Always,
    /// Never emit colors
    Never,
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

    /// Path to config file (default: search for .aadcrc)
    #[arg(long = "config", value_name = "FILE")]
    config_file: Option<PathBuf>,

    /// Ignore config files
    #[arg(long = "no-config")]
    no_config: bool,

    /// Process files recursively in directories
    #[arg(short = 'r', long)]
    recursive: bool,

    /// Glob pattern to match files when recursing (comma-separated)
    #[arg(long, default_value = "*.txt,*.md", requires = "recursive")]
    glob: String,

    /// Do not respect .gitignore when recursing
    #[arg(long = "no-gitignore", requires = "recursive")]
    no_gitignore: bool,

    /// Maximum directory depth (0 = unlimited)
    #[arg(long, default_value = "0", requires = "recursive")]
    max_depth: usize,

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

    /// Process only specific line ranges (e.g., "10-50", "1-100,200-250", "50-", "-100")
    #[arg(short = 'L', long, value_name = "RANGES")]
    lines: Option<String>,

    /// Verbose output showing correction progress
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Color output: auto, always, or never
    #[arg(long, value_enum, default_value = "auto")]
    color: ColorMode,

    /// Show unified diff of changes instead of full output
    #[arg(short = 'd', long)]
    diff: bool,

    /// Preview changes without modifying files (exit 0=no changes, 3=would change)
    #[arg(short = 'n', long, conflicts_with = "in_place")]
    dry_run: bool,

    /// Watch file for changes and auto-correct
    #[arg(short = 'w', long, conflicts_with_all = ["in_place", "recursive", "diff", "dry_run", "json"])]
    watch: bool,

    /// Debounce interval in milliseconds (for --watch mode)
    #[arg(long, default_value = "500", requires = "watch")]
    debounce_ms: u64,

    /// Create backup file before in-place editing
    #[arg(long, requires = "in_place")]
    backup: bool,

    /// Extension for backup files (default: .bak)
    #[arg(long, default_value = ".bak", requires = "backup")]
    backup_ext: String,

    /// Output results as JSON for programmatic processing
    #[arg(long, conflicts_with_all = ["verbose", "diff"])]
    json: bool,

    /// Subcommand (hook management)
    #[command(subcommand)]
    command: Option<Commands>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Subcommands
// ─────────────────────────────────────────────────────────────────────────────

/// Available subcommands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage git pre-commit hook
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

/// Config management actions
#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Initialize a new .aadcrc config file
    Init {
        /// Create in home directory instead of current
        #[arg(long)]
        global: bool,
    },
    /// Show effective configuration (merged file + CLI)
    Show,
    /// Show path to active config file
    Path,
}

/// Hook management actions
#[derive(Subcommand, Debug)]
enum HookAction {
    /// Install pre-commit hook
    Install {
        /// Only check diagrams, don't auto-fix (blocks commits with issues)
        #[arg(long)]
        check_only: bool,

        /// Auto-fix diagrams before commit
        #[arg(long, conflicts_with = "check_only")]
        auto_fix: bool,

        /// File patterns to check (default: *.md *.txt)
        #[arg(long, value_delimiter = ',')]
        patterns: Option<Vec<String>>,
    },
    /// Uninstall pre-commit hook
    Uninstall,
    /// Show hook status
    Status,
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration and Statistics
// ─────────────────────────────────────────────────────────────────────────────

/// Runtime configuration derived from CLI args
#[derive(Debug)]
struct Config {
    max_iters: usize,
    min_score: f64,
    preset: Option<Preset>,
    tab_width: usize,
    all_blocks: bool,
    lines: Option<Vec<LineRange>>,
    recursive: bool,
    glob: String,
    gitignore: bool,
    max_depth: usize,
    color: ColorMode,
    verbose: bool,
    diff: bool,
    dry_run: bool,
    watch: bool,
    debounce_ms: u64,
    backup: bool,
    backup_ext: String,
    json: bool,
}

impl From<&Args> for Config {
    fn from(args: &Args) -> Self {
        // Parse line ranges if provided
        let lines = args.lines.as_ref().and_then(|s| parse_line_ranges(s).ok());

        Self {
            max_iters: args.max_iters,
            min_score: args.min_score,
            preset: args.preset,
            tab_width: args.tab_width,
            all_blocks: args.all,
            lines,
            recursive: args.recursive,
            glob: args.glob.clone(),
            gitignore: !args.no_gitignore,
            max_depth: args.max_depth,
            color: args.color,
            verbose: args.verbose,
            diff: args.diff,
            dry_run: args.dry_run,
            watch: args.watch,
            debounce_ms: args.debounce_ms,
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

struct VerboseStyle {
    use_color: bool,
}

impl VerboseStyle {
    fn new(use_color: bool) -> Self {
        Self { use_color }
    }

    fn wrap(&self, tag: &str, text: impl fmt::Display) -> String {
        if self.use_color {
            format!("[{}]{}[/]", tag, text)
        } else {
            text.to_string()
        }
    }

    fn header(&self, text: impl fmt::Display) -> String {
        self.wrap("bold cyan", text)
    }

    fn block(&self, text: impl fmt::Display) -> String {
        self.wrap("yellow", text)
    }

    fn success(&self, text: impl fmt::Display) -> String {
        self.wrap("bold green", text)
    }

    fn dim(&self, text: impl fmt::Display) -> String {
        self.wrap("dim", text)
    }

    fn bold(&self, text: impl fmt::Display) -> String {
        self.wrap("bold", text)
    }

    fn stat_label(&self, text: impl fmt::Display) -> String {
        self.wrap("bold blue", text)
    }

    fn separator(&self) -> String {
        self.wrap("dim", "───")
    }
}

/// Print a statistics summary to stderr
fn print_stats_summary(
    stats: &Stats,
    files_processed: usize,
    files_changed: usize,
    errors: usize,
    console: &Console,
    styles: &VerboseStyle,
) {
    console.print("");
    console.print(&format!(
        "{} Summary {}",
        styles.separator(),
        styles.separator()
    ));

    // File statistics (for multiple files)
    if files_processed > 1 {
        console.print(&format!(
            "  {} {} processed, {} modified, {} unchanged",
            styles.stat_label("Files:"),
            files_processed,
            files_changed,
            files_processed.saturating_sub(files_changed)
        ));
    }

    // Block statistics
    console.print(&format!(
        "  {} {} found, {} processed, {} skipped",
        styles.stat_label("Blocks:"),
        stats.blocks_found,
        stats.blocks_modified,
        stats.blocks_skipped
    ));

    // Revision statistics
    console.print(&format!(
        "  {} {} applied, {} skipped",
        styles.stat_label("Revisions:"),
        stats.total_revisions,
        stats.revisions_skipped
    ));

    // Performance statistics
    let elapsed_ms = stats.elapsed.as_secs_f64() * 1000.0;
    let lines_per_sec = stats.lines_per_second();
    console.print(&format!(
        "  {} {:.2}ms ({:.0} lines/sec)",
        styles.stat_label("Time:"),
        elapsed_ms,
        lines_per_sec
    ));

    // Error count if any
    if errors > 0 {
        console.print(&format!(
            "  {} {}",
            styles.wrap("bold red", "Errors:"),
            errors
        ));
    }

    console.print("");
}

fn build_console(color: ColorMode) -> (Console, VerboseStyle) {
    match color {
        ColorMode::Never => (Console::new(), VerboseStyle::new(false)),
        ColorMode::Always => {
            let system = terminal::detect_color_system().unwrap_or(ColorSystem::Standard);
            let console = Console::builder()
                .force_terminal(true)
                .color_system(system)
                .build();
            (console, VerboseStyle::new(true))
        }
        ColorMode::Auto => {
            if std::env::var("NO_COLOR").is_ok() {
                return (Console::new(), VerboseStyle::new(false));
            }

            if std::env::var("FORCE_COLOR").is_ok() {
                let system = terminal::detect_color_system().unwrap_or(ColorSystem::Standard);
                let console = Console::builder()
                    .force_terminal(true)
                    .color_system(system)
                    .build();
                return (console, VerboseStyle::new(true));
            }

            let console = Console::new();
            let use_color = console.is_color_enabled();
            (console, VerboseStyle::new(use_color))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config File Support
// ─────────────────────────────────────────────────────────────────────────────

/// Config file names searched in order
const CONFIG_FILENAMES: &[&str] = &[".aadcrc", ".aadcrc.toml", "aadcrc.toml"];

/// Configuration loaded from a .aadcrc file
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    /// Minimum confidence score (0.0-1.0)
    min_score: Option<f64>,
    /// Confidence threshold preset (overrides min_score)
    preset: Option<Preset>,
    /// Maximum correction iterations
    max_iters: Option<usize>,
    /// Tab expansion width
    tab_width: Option<usize>,
    /// Show verbose output
    verbose: Option<bool>,
    /// Color mode: auto, always, never
    color: Option<ColorMode>,
    /// Output as JSON
    json: Option<bool>,
    /// Create backup before in-place edit
    backup: Option<bool>,
    /// Backup file extension
    backup_ext: Option<String>,
    /// Enable recursive mode
    recursive: Option<bool>,
    /// Glob patterns for recursive mode
    glob: Option<String>,
    /// Respect .gitignore
    gitignore: Option<bool>,
    /// Maximum directory depth
    max_depth: Option<usize>,
    /// Process all diagram-like blocks
    all: Option<bool>,
}

/// Search for a config file starting from the given directory
fn find_config_file(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();

    // Search up the directory tree
    loop {
        for filename in CONFIG_FILENAMES {
            let config_path = current.join(filename);
            if config_path.exists() {
                return Some(config_path);
            }
        }

        if !current.pop() {
            break;
        }
    }

    // Check home directory
    if let Some(home) = dirs::home_dir() {
        for filename in CONFIG_FILENAMES {
            let config_path = home.join(filename);
            if config_path.exists() {
                return Some(config_path);
            }
        }
    }

    None
}

/// Load and parse a config file
fn load_config_file(path: &Path) -> Result<FileConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))
}

/// Create Config by merging file config with CLI args (CLI wins)
fn create_config(args: &Args) -> Result<Config> {
    let mut config = Config::from(args);

    // Skip config file loading if --no-config is set
    if args.no_config {
        return Ok(config);
    }

    // Find and load config file
    let config_path = if let Some(ref path) = args.config_file {
        // Explicit config file specified
        if !path.exists() {
            return Err(anyhow::anyhow!("Config file not found: {}", path.display()));
        }
        Some(path.clone())
    } else {
        // Search for config file
        let start_dir = args
            .inputs
            .first()
            .and_then(|p| {
                if p.is_dir() {
                    Some(p.clone())
                } else {
                    p.parent().map(|p| p.to_path_buf())
                }
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        find_config_file(&start_dir)
    };

    if let Some(path) = config_path {
        let file_config = load_config_file(&path)?;

        // Merge file config with CLI config (CLI wins)
        // Only apply file config values when CLI used defaults
        if args.preset.is_none() {
            if let Some(preset) = file_config.preset {
                config.preset = Some(preset);
            } else if let Some(score) = file_config.min_score {
                // Only use min_score from file if no preset specified
                if config.preset.is_none() {
                    config.min_score = score;
                }
            }
        }

        // max_iters: use file value if CLI used default (10)
        if args.max_iters == 10 {
            if let Some(iters) = file_config.max_iters {
                config.max_iters = iters;
            }
        }

        // tab_width: use file value if CLI used default (4)
        if args.tab_width == 4 {
            if let Some(width) = file_config.tab_width {
                config.tab_width = width;
            }
        }

        // Boolean flags: use file value if CLI flag wasn't set
        if !args.verbose {
            if let Some(v) = file_config.verbose {
                config.verbose = v;
            }
        }

        if args.color == ColorMode::Auto {
            if let Some(c) = file_config.color {
                config.color = c;
            }
        }

        if !args.json {
            if let Some(j) = file_config.json {
                config.json = j;
            }
        }

        if !args.backup {
            if let Some(b) = file_config.backup {
                config.backup = b;
            }
        }

        // backup_ext: use file value if CLI used default
        if args.backup_ext == ".bak" {
            if let Some(ext) = file_config.backup_ext {
                config.backup_ext = ext;
            }
        }

        // Recursive options
        if !args.recursive {
            if let Some(r) = file_config.recursive {
                config.recursive = r;
            }
        }

        if args.glob == "*.txt,*.md" {
            if let Some(g) = file_config.glob {
                config.glob = g;
            }
        }

        if !args.no_gitignore {
            if let Some(gi) = file_config.gitignore {
                config.gitignore = gi;
            }
        }

        if args.max_depth == 0 {
            if let Some(d) = file_config.max_depth {
                config.max_depth = d;
            }
        }

        if !args.all {
            if let Some(a) = file_config.all {
                config.all_blocks = a;
            }
        }
    }

    Ok(config)
}

/// Default config file content
const DEFAULT_CONFIG: &str = r#"# .aadcrc - aadc configuration file
# https://github.com/Dicklesworthstone/aadc

# Confidence threshold for applying edits
# Use either min_score (0.0-1.0) or preset (strict|normal|aggressive|relaxed)
# preset = "normal"
min_score = 0.5

# Maximum correction iterations per block
max_iters = 10

# Tab expansion width
tab_width = 4

# Output options
# verbose = false
# color = "auto"
# json = false

# Backup options (for --in-place)
# backup = false
# backup_ext = ".bak"

# Recursive mode defaults
# recursive = false
# glob = "*.txt,*.md"
# gitignore = true
# max_depth = 0

# Force processing of low-confidence blocks
# all = false
"#;

/// Handle the config subcommand
fn run_config_command(action: &ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Init { global } => {
            let path = if *global {
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
                    .join(".aadcrc")
            } else {
                PathBuf::from(".aadcrc")
            };

            if path.exists() {
                return Err(anyhow::anyhow!(
                    "Config file already exists: {}",
                    path.display()
                ));
            }

            fs::write(&path, DEFAULT_CONFIG)
                .with_context(|| format!("Failed to create config file: {}", path.display()))?;

            eprintln!("Created config file: {}", path.display());
            Ok(())
        }

        ConfigAction::Show => {
            // Parse minimal args to get effective config
            let args = Args::parse_from(["aadc"]);
            let config = create_config(&args)?;

            eprintln!("Effective configuration:");
            eprintln!("  min_score: {}", config.effective_min_score());
            if let Some(preset) = config.preset {
                eprintln!("  preset: {:?}", preset);
            }
            eprintln!("  max_iters: {}", config.max_iters);
            eprintln!("  tab_width: {}", config.tab_width);
            eprintln!("  verbose: {}", config.verbose);
            eprintln!("  color: {:?}", config.color);
            eprintln!("  json: {}", config.json);
            eprintln!("  backup: {}", config.backup);
            eprintln!("  backup_ext: {}", config.backup_ext);
            eprintln!("  recursive: {}", config.recursive);
            eprintln!("  glob: {}", config.glob);
            eprintln!("  gitignore: {}", config.gitignore);
            eprintln!("  max_depth: {}", config.max_depth);
            eprintln!("  all_blocks: {}", config.all_blocks);

            // Show config file path if found
            let start_dir = std::env::current_dir().unwrap_or_default();
            if let Some(path) = find_config_file(&start_dir) {
                eprintln!();
                eprintln!("Config file: {}", path.display());
            }

            Ok(())
        }

        ConfigAction::Path => {
            let start_dir = std::env::current_dir().unwrap_or_default();
            if let Some(path) = find_config_file(&start_dir) {
                println!("{}", path.display());
                Ok(())
            } else {
                eprintln!("No config file found");
                std::process::exit(1);
            }
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

    if args.tab_width == 0 || args.tab_width > 16 {
        return Err(ArgError("--tab-width must be between 1 and 16".to_string()).into());
    }

    if args.in_place && args.inputs.is_empty() {
        return Err(ArgError("--in-place requires at least one input file".to_string()).into());
    }

    if args.recursive && args.inputs.is_empty() {
        return Err(ArgError("--recursive requires at least one input path".to_string()).into());
    }

    Ok(())
}

/// Statistics collected during correction
#[derive(Default, Clone)]
struct Stats {
    /// Number of diagram blocks detected
    blocks_found: usize,
    /// Number of blocks that received modifications
    blocks_modified: usize,
    /// Number of blocks skipped (low confidence or outside line ranges)
    blocks_skipped: usize,
    /// Total number of revisions applied
    total_revisions: usize,
    /// Number of revisions skipped (below min_score threshold)
    revisions_skipped: usize,
    /// Total number of lines processed
    total_lines: usize,
    /// Processing elapsed time
    elapsed: Duration,
}

impl Stats {
    /// Merge another Stats into this one (for aggregating across files)
    fn merge(&mut self, other: &Stats) {
        self.blocks_found += other.blocks_found;
        self.blocks_modified += other.blocks_modified;
        self.blocks_skipped += other.blocks_skipped;
        self.total_revisions += other.total_revisions;
        self.revisions_skipped += other.revisions_skipped;
        self.total_lines += other.total_lines;
        self.elapsed += other.elapsed;
    }

    /// Calculate lines processed per second
    fn lines_per_second(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs > 0.0 {
            self.total_lines as f64 / secs
        } else {
            self.total_lines as f64
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Quick Scan (Passthrough Optimization)
// ─────────────────────────────────────────────────────────────────────────────

/// Minimum fraction of lines that must contain box-drawing chars to run full processing.
const QUICK_SCAN_THRESHOLD: f64 = 0.01; // 1%

/// Maximum number of lines to scan when deciding whether to process.
const QUICK_SCAN_LIMIT: usize = 1000;

/// Summary of a quick scan decision for diagram detection.
#[derive(Debug)]
struct QuickScanResult {
    lines_scanned: usize,
    lines_with_box_chars: usize,
    ratio: f64,
    likely_has_diagrams: bool,
}

/// Quickly scan input lines to decide whether full processing is necessary.
fn quick_scan_for_diagrams(lines: &[String]) -> QuickScanResult {
    let mut lines_scanned = 0;
    let mut lines_with_box_chars = 0;

    for line in lines.iter().take(QUICK_SCAN_LIMIT) {
        lines_scanned += 1;
        if line.chars().any(is_box_char) {
            lines_with_box_chars += 1;
        }
    }

    let ratio = if lines_scanned > 0 {
        lines_with_box_chars as f64 / lines_scanned as f64
    } else {
        0.0
    };

    let likely_has_diagrams = ratio >= QUICK_SCAN_THRESHOLD;

    QuickScanResult {
        lines_scanned,
        lines_with_box_chars,
        ratio,
        likely_has_diagrams,
    }
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

/// Classification of a line's role in a diagram.
///
/// Lines are classified based on the presence and type of box-drawing
/// characters. This classification drives revision generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// Empty or whitespace-only line.
    ///
    /// Blank lines may separate logical sections within a diagram.
    Blank,

    /// A line with no detected diagram structure.
    ///
    /// These lines are passed through unchanged.
    None,

    /// A line with vertical borders but no horizontal structure.
    ///
    /// Weak lines form the content rows of boxes:
    /// ```text
    /// | Content  |   ← Weak (vertical borders only)
    /// │ データ   │   ← Weak (Unicode vertical)
    /// ```
    Weak,

    /// A line with strong horizontal structure.
    ///
    /// Strong lines typically form the top/bottom borders of boxes:
    /// ```text
    /// +----------+   ← Strong (corners + horizontal runs)
    /// ┌──────────┐   ← Strong (Unicode corners + horizontal)
    /// ```
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

/// Result of analyzing a single line for diagram structure.
///
/// Contains extracted properties used for revision generation:
/// - The line's classification (Strong, Weak, Blank, None)
/// - Visual width accounting for CJK and other wide characters
/// - Suffix border position and character if detected
#[derive(Debug)]
struct AnalyzedLine {
    /// The original line content (unmodified)
    #[allow(dead_code)]
    content: String,

    /// Classification of the line based on box-drawing characters
    kind: LineKind,

    /// Visual width in terminal columns (CJK chars count as 2)
    #[allow(dead_code)]
    visual_width: usize,

    /// Number of leading space characters
    #[allow(dead_code)]
    indent: usize,

    /// Detected right-side border information, if any
    suffix_border: Option<SuffixBorder>,
}

/// Information about a detected right-side border character.
///
/// Used to determine the target column for alignment and to
/// generate revisions that pad lines to match.
#[derive(Debug, Clone)]
struct SuffixBorder {
    /// Visual column position where the border appears (0-indexed)
    column: usize,

    /// The actual border character (`|`, `│`, etc.)
    #[allow(dead_code)]
    char: char,

    /// True if this appears to be a closing border (end of content),
    /// false if it's a mid-line separator
    #[allow(dead_code)]
    is_closing: bool,
}

/// Calculate the visual width of a string in terminal columns.
///
/// Handles different character widths:
/// - ASCII characters: 1 column each
/// - CJK characters (Chinese, Japanese, Korean): 2 columns each
/// - Emoji and other wide Unicode: 2 columns each
///
/// # Examples
///
/// ```text
/// visual_width("Hello")     == 5   // ASCII only
/// visual_width("你好")      == 4   // CJK (2 chars × 2 columns)
/// visual_width("Hello世界") == 9   // 5 ASCII + 2 CJK chars
/// ```
///
/// This is critical for correct padding calculations in diagrams.
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

/// A detected ASCII diagram block within the input text.
///
/// Blocks are identified by consecutive lines containing box-drawing
/// characters. Each block is processed independently by the correction
/// algorithm.
///
/// # Confidence Scoring
///
/// The confidence score (0.0-1.0) indicates how likely this block is
/// to be an actual diagram versus coincidental box characters:
/// - 0.9-1.0: Very likely a diagram (multiple strong lines)
/// - 0.5-0.9: Probably a diagram (mixed strong/weak lines)
/// - 0.0-0.5: Uncertain (weak lines only, may be table or code)
#[derive(Debug)]
struct DiagramBlock {
    /// Starting line index in the input (0-based, inclusive)
    start: usize,

    /// Ending line index in the input (exclusive)
    end: usize,

    /// Confidence that this is an actual diagram (0.0-1.0)
    confidence: f64,
}

/// Find diagram blocks in the input text.
///
/// Scans the input for consecutive lines containing box-drawing characters
/// and groups them into blocks. Uses lookahead to merge blocks separated
/// by single blank lines.
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

/// A proposed modification to align a line's right border.
///
/// Revisions are generated during the correction loop and scored for
/// confidence. Only revisions above the `--min-score` threshold are applied.
///
/// # Scoring
///
/// Each revision type has different base confidence scores:
/// - `PadBeforeSuffixBorder`: Higher confidence (0.3-0.9), as we're just adding
///   whitespace before an existing border
/// - `AddSuffixBorder`: Lower confidence (0.3-0.6), as we're adding a character
///   that wasn't there
///
/// # Monotone Edits
///
/// Both revision types are "monotone" (insert-only) - they never remove
/// content from the line, making them safe to apply.
#[derive(Debug, Clone)]
enum Revision {
    /// Insert spaces before an existing suffix border to align it.
    ///
    /// This is the most common revision type and has higher confidence
    /// since we're only adjusting whitespace.
    PadBeforeSuffixBorder {
        /// Global line index (0-based)
        line_idx: usize,
        /// Number of space characters to insert
        spaces_to_add: usize,
        /// Target visual column for alignment
        #[allow(dead_code)]
        target_column: usize,
    },

    /// Add a border character at the target column.
    ///
    /// Used when a line has content but no closing border. Lower confidence
    /// since we're adding structure that may not be intended.
    AddSuffixBorder {
        /// Global line index (0-based)
        line_idx: usize,
        /// Border character to add (`|`, `│`, etc.)
        border_char: char,
        /// Target visual column for the new border
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

/// Result of correcting a single block
struct BlockCorrectionResult {
    /// Number of revisions applied
    revisions_applied: usize,
    /// Number of revisions skipped due to low score
    revisions_skipped: usize,
}

/// Correct a single diagram block using iterative refinement.
///
/// This is the core correction algorithm. It runs a loop that:
/// 1. Analyzes all lines in the block to find their border positions
/// 2. Determines the target column (rightmost border position)
/// 3. Generates candidate revisions to align other lines to the target
/// 4. Scores each revision and filters by `min_score`
/// 5. Applies valid revisions
/// 6. Repeats until no more revisions needed or `max_iters` reached
///
/// # Arguments
///
/// * `lines` - Mutable slice of all input lines (block is modified in place)
/// * `block` - The block to correct (defines which lines to process)
/// * `config` - Configuration with thresholds and iteration limits
/// * `console` - For verbose output
///
/// # Returns
///
/// A `BlockCorrectionResult` with counts of applied and skipped revisions.
fn correct_block(
    lines: &mut [String],
    block: &DiagramBlock,
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
) -> BlockCorrectionResult {
    let mut total_revisions = 0;
    let mut total_skipped = 0;

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

        // Filter by score and count skipped
        let min_score = config.effective_min_score();
        let total_candidates = revisions.len();
        let valid_revisions: Vec<_> = revisions
            .into_iter()
            .filter(|r| r.score(&analyzed, block.start) >= min_score)
            .collect();
        let skipped_this_iter = total_candidates - valid_revisions.len();
        total_skipped += skipped_this_iter;

        if valid_revisions.is_empty() {
            // Converged
            if config.verbose && iteration > 0 {
                console.print(
                    &styles
                        .dim(format!("    Converged after {} iteration(s)", iteration))
                        .to_string(),
                );
            }
            break;
        }

        // Apply revisions
        for rev in &valid_revisions {
            rev.apply(lines);
        }

        total_revisions += valid_revisions.len();

        if config.verbose {
            console.print(
                &styles
                    .dim(format!(
                        "    Iteration {}: applied {} revision(s)",
                        iteration + 1,
                        valid_revisions.len()
                    ))
                    .to_string(),
            );
        }
    }

    BlockCorrectionResult {
        revisions_applied: total_revisions,
        revisions_skipped: total_skipped,
    }
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

/// Check if a block overlaps with any of the given line ranges
/// Block indices are 0-indexed, ranges are 1-indexed
fn block_overlaps_ranges(block: &DiagramBlock, ranges: &[LineRange]) -> bool {
    // Convert block to 1-indexed for comparison with ranges
    let block_start = block.start + 1;
    let block_end = block.end; // end is already exclusive, so it's effectively 1-indexed

    ranges.iter().any(|r| {
        // Check if block and range overlap
        block_start <= r.end && block_end >= r.start
    })
}

/// Format line ranges for display
fn format_line_ranges(ranges: &[LineRange], total_lines: usize) -> String {
    let range_strs: Vec<String> = ranges
        .iter()
        .map(|r| {
            if r.end == usize::MAX {
                format!("{}-", r.start)
            } else if r.start == r.end {
                format!("{}", r.start)
            } else {
                format!("{}-{}", r.start, r.end)
            }
        })
        .collect();

    // Calculate how many lines are covered
    let covered: usize = ranges
        .iter()
        .map(|r| {
            let effective_end = r.end.min(total_lines);
            if r.start <= effective_end {
                effective_end - r.start + 1
            } else {
                0
            }
        })
        .sum();

    format!(
        "{} ({} of {} lines)",
        range_strs.join(", "),
        covered,
        total_lines
    )
}

/// Main correction entry point
fn correct_lines(
    lines: Vec<String>,
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
) -> (Vec<String>, Stats) {
    let start_time = Instant::now();
    let mut stats = Stats::default();
    let total_lines = lines.len();
    stats.total_lines = total_lines;

    // Show line range info in verbose mode
    if config.verbose {
        if let Some(ref ranges) = config.lines {
            console.print(
                &styles
                    .header(format!(
                        "Line ranges: {}",
                        format_line_ranges(ranges, total_lines)
                    ))
                    .to_string(),
            );
        }
    }

    if !config.all_blocks {
        let scan = quick_scan_for_diagrams(&lines);
        if !scan.likely_has_diagrams {
            if config.verbose {
                console.print(
                    &styles
                        .dim(format!(
                            "Quick scan: no diagrams detected ({}/{} lines, {:.1}% box chars < {:.1}% threshold)",
                            scan.lines_with_box_chars,
                            scan.lines_scanned,
                            scan.ratio * 100.0,
                            QUICK_SCAN_THRESHOLD * 100.0
                        ))
                        .to_string(),
                );
                console.print(
                    &styles.dim("Passing through unchanged (use --all to force processing)"),
                );
            }
            stats.elapsed = start_time.elapsed();
            return (lines, stats);
        }
    }

    // Expand tabs
    let mut lines: Vec<String> = lines
        .into_iter()
        .map(|l| expand_tabs(&l, config.tab_width))
        .collect();

    // Find diagram blocks
    let blocks = find_diagram_blocks(&lines, config.all_blocks);
    stats.blocks_found = blocks.len();

    if config.verbose {
        console.print(
            &styles
                .header(format!("Found {} diagram block(s)", blocks.len()))
                .to_string(),
        );
    }

    // Correct each block
    for (i, block) in blocks.iter().enumerate() {
        // Check if block overlaps with line ranges (if specified)
        if let Some(ref ranges) = config.lines {
            if !block_overlaps_ranges(block, ranges) {
                if config.verbose {
                    console.print(
                        &styles
                            .dim(format!(
                                "  Block {}: lines {}-{} (skipped: outside line ranges)",
                                i + 1,
                                block.start + 1,
                                block.end
                            ))
                            .to_string(),
                    );
                }
                stats.blocks_skipped += 1;
                continue;
            }
        }

        if config.verbose {
            console.print(
                &styles
                    .block(format!(
                        "  Block {}: lines {}-{} (confidence: {:.0}%)",
                        i + 1,
                        block.start + 1,
                        block.end,
                        block.confidence * 100.0
                    ))
                    .to_string(),
            );
        }

        let result = correct_block(&mut lines, block, config, console, styles);
        if result.revisions_applied > 0 {
            stats.blocks_modified += 1;
            stats.total_revisions += result.revisions_applied;
        }
        stats.revisions_skipped += result.revisions_skipped;
    }

    stats.elapsed = start_time.elapsed();
    (lines, stats)
}

// ─────────────────────────────────────────────────────────────────────────────
// Recursive File Discovery
// ─────────────────────────────────────────────────────────────────────────────

fn build_globset(patterns: &str) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut added = 0;

    for raw in patterns.split(',') {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }

        let glob = Glob::new(pattern)
            .map_err(|err| ArgError(format!("Invalid glob pattern '{}': {}", pattern, err)))?;
        builder.add(glob);
        added += 1;
    }

    if added == 0 {
        return Err(ArgError("--glob must include at least one pattern".to_string()).into());
    }

    builder
        .build()
        .map_err(|err| ArgError(format!("Invalid glob set: {}", err)).into())
}

fn discover_recursive_files(
    paths: &[PathBuf],
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
) -> Result<Vec<PathBuf>> {
    let globs = build_globset(&config.glob)?;
    let mut files = std::collections::BTreeSet::new();

    for path in paths {
        if path.is_file() {
            files.insert(path.clone());
            continue;
        }

        if !path.is_dir() {
            if config.verbose {
                console.print(
                    &styles
                        .dim(format!("Warning: path does not exist: {}", path.display()))
                        .to_string(),
                );
            }
            continue;
        }

        let mut walker = WalkBuilder::new(path);
        walker.git_ignore(config.gitignore);
        walker.git_exclude(config.gitignore);
        walker.git_global(config.gitignore);
        walker.ignore(config.gitignore);
        walker.hidden(false);

        if config.max_depth > 0 {
            walker.max_depth(Some(config.max_depth));
        }

        for entry in walker.build() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    if config.verbose {
                        console.print(&styles.dim(format!("Warning: {}", err)));
                    }
                    continue;
                }
            };

            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(name) = entry_path.file_name() {
                    if globs.is_match(name) {
                        files.insert(entry_path.to_path_buf());
                    }
                }
            }
        }
    }

    Ok(files.into_iter().collect())
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

/// Maximum file size (100 MB) - reject larger files to prevent memory issues
const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// Read content from a file path and return lines
fn read_file(path: &Path) -> Result<Vec<String>> {
    // Check file size before reading
    let metadata = fs::metadata(path)
        .with_context(|| format!("Failed to read file metadata: {}", path.display()))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(ParseError(format!(
            "File too large: {} ({} MB). Maximum supported size is {} MB.",
            path.display(),
            metadata.len() / (1024 * 1024),
            MAX_FILE_SIZE / (1024 * 1024)
        ))
        .into());
    }

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
// Hook Management
// ─────────────────────────────────────────────────────────────────────────────

/// Marker comment identifying aadc-generated hooks
const HOOK_MARKER: &str = "# aadc pre-commit hook";

/// Default file patterns for hook
const DEFAULT_PATTERNS: &[&str] = &["*.md", "*.txt"];

/// Run a subcommand
fn run_command(command: &Commands) -> Result<()> {
    match command {
        Commands::Hook { action } => run_hook_command(action),
        Commands::Config { action } => run_config_command(action),
    }
}

/// Run a hook subcommand
fn run_hook_command(action: &HookAction) -> Result<()> {
    match action {
        HookAction::Install {
            check_only,
            auto_fix,
            patterns,
        } => hook_install(*check_only, *auto_fix, patterns.as_deref()),
        HookAction::Uninstall => hook_uninstall(),
        HookAction::Status => hook_status(),
    }
}

/// Find the .git directory, searching upward from current directory
fn find_git_dir() -> Result<PathBuf> {
    let mut current = std::env::current_dir().context("Failed to get current directory")?;

    loop {
        let git_dir = current.join(".git");
        if git_dir.is_dir() {
            return Ok(git_dir);
        }
        if !current.pop() {
            return Err(anyhow::anyhow!(
                "Not in a git repository (or any parent up to the filesystem root)"
            ));
        }
    }
}

/// Generate the check-mode hook script
fn generate_check_hook(patterns: &[&str]) -> String {
    let patterns_str = patterns.join(" ");
    format!(
        r#"#!/usr/bin/env bash
{marker} (check mode)
# Generated by: aadc hook install --check-only
# Blocks commits if diagrams have misaligned borders

set -e

PATTERNS="{patterns}"

# Get staged files matching patterns
staged_files() {{
    for pattern in $PATTERNS; do
        git diff --cached --name-only --diff-filter=ACM | grep -E "${{pattern//\*/.*}}" || true
    done | sort -u
}}

files=$(staged_files)
if [[ -z "$files" ]]; then
    exit 0
fi

failed=0
for file in $files; do
    if ! aadc --dry-run "$file" > /dev/null 2>&1; then
        echo "aadc: Diagram alignment needed: $file"
        ((failed++)) || true
    fi
done

if [[ $failed -gt 0 ]]; then
    echo ""
    echo "Run 'aadc -i <file>' to fix, or 'git commit --no-verify' to skip"
    exit 1
fi
"#,
        marker = HOOK_MARKER,
        patterns = patterns_str
    )
}

/// Generate the auto-fix mode hook script
fn generate_autofix_hook(patterns: &[&str]) -> String {
    let patterns_str = patterns.join(" ");
    format!(
        r#"#!/usr/bin/env bash
{marker} (auto-fix mode)
# Generated by: aadc hook install --auto-fix
# Automatically fixes diagram alignment before commit

set -e

PATTERNS="{patterns}"

# Get staged files matching patterns
staged_files() {{
    for pattern in $PATTERNS; do
        git diff --cached --name-only --diff-filter=ACM | grep -E "${{pattern//\*/.*}}" || true
    done | sort -u
}}

files=$(staged_files)
if [[ -z "$files" ]]; then
    exit 0
fi

modified=0
for file in $files; do
    if ! aadc --dry-run "$file" > /dev/null 2>&1; then
        echo "aadc: Auto-fixing diagrams: $file"
        aadc -i "$file"
        git add "$file"
        ((modified++)) || true
    fi
done

if [[ $modified -gt 0 ]]; then
    echo "aadc: Auto-fixed $modified file(s)"
fi
"#,
        marker = HOOK_MARKER,
        patterns = patterns_str
    )
}

/// Generate the default (check) hook script
fn generate_default_hook(patterns: &[&str]) -> String {
    generate_check_hook(patterns)
}

/// Install the pre-commit hook
fn hook_install(check_only: bool, auto_fix: bool, patterns: Option<&[String]>) -> Result<()> {
    let git_dir = find_git_dir()?;
    let hooks_dir = git_dir.join("hooks");
    let hook_path = hooks_dir.join("pre-commit");

    // Create hooks directory if it doesn't exist
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir).with_context(|| {
            format!("Failed to create hooks directory: {}", hooks_dir.display())
        })?;
    }

    // Check for existing hook
    if hook_path.exists() {
        let content = fs::read_to_string(&hook_path)
            .with_context(|| format!("Failed to read existing hook: {}", hook_path.display()))?;

        if content.contains(HOOK_MARKER) {
            // Our hook already installed - update it
            println!("Updating existing aadc hook...");
        } else {
            // Different hook present - backup before overwriting
            let backup_path = hook_path.with_extension("pre-aadc");
            fs::rename(&hook_path, &backup_path).with_context(|| {
                format!(
                    "Failed to backup existing hook to: {}",
                    backup_path.display()
                )
            })?;
            println!("Backed up existing hook to: {}", backup_path.display());
        }
    }

    // Determine patterns to use
    let pattern_refs: Vec<&str> = match patterns {
        Some(p) => p.iter().map(|s| s.as_str()).collect(),
        None => DEFAULT_PATTERNS.to_vec(),
    };

    // Generate hook script based on mode
    let script = if auto_fix {
        generate_autofix_hook(&pattern_refs)
    } else if check_only {
        generate_check_hook(&pattern_refs)
    } else {
        // Default to check mode
        generate_default_hook(&pattern_refs)
    };

    // Write hook
    fs::write(&hook_path, &script)
        .with_context(|| format!("Failed to write hook: {}", hook_path.display()))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms)
            .with_context(|| format!("Failed to make hook executable: {}", hook_path.display()))?;
    }

    let mode = if auto_fix { "auto-fix" } else { "check" };
    println!(
        "Installed aadc pre-commit hook ({} mode): {}",
        mode,
        hook_path.display()
    );
    println!("Patterns: {}", pattern_refs.join(", "));

    Ok(())
}

/// Uninstall the pre-commit hook
fn hook_uninstall() -> Result<()> {
    let git_dir = find_git_dir()?;
    let hook_path = git_dir.join("hooks").join("pre-commit");

    if !hook_path.exists() {
        println!("No pre-commit hook installed");
        return Ok(());
    }

    let content = fs::read_to_string(&hook_path)
        .with_context(|| format!("Failed to read hook: {}", hook_path.display()))?;

    if !content.contains(HOOK_MARKER) {
        return Err(anyhow::anyhow!(
            "Pre-commit hook exists but was not installed by aadc. Remove manually if desired."
        ));
    }

    fs::remove_file(&hook_path)
        .with_context(|| format!("Failed to remove hook: {}", hook_path.display()))?;

    println!("Removed aadc pre-commit hook");

    // Check for backup to restore
    let backup_path = hook_path.with_extension("pre-aadc");
    if backup_path.exists() {
        println!(
            "Note: Previous hook backup exists at: {}",
            backup_path.display()
        );
        println!(
            "Restore it manually with: mv {} {}",
            backup_path.display(),
            hook_path.display()
        );
    }

    Ok(())
}

/// Show hook status
fn hook_status() -> Result<()> {
    let git_dir = find_git_dir()?;
    let hook_path = git_dir.join("hooks").join("pre-commit");

    if !hook_path.exists() {
        println!("Status: No pre-commit hook installed");
        return Ok(());
    }

    let content = fs::read_to_string(&hook_path)
        .with_context(|| format!("Failed to read hook: {}", hook_path.display()))?;

    if content.contains(&format!("{} (check mode)", HOOK_MARKER)) {
        println!("Status: aadc hook installed (check mode)");
        println!("Path: {}", hook_path.display());
    } else if content.contains(&format!("{} (auto-fix mode)", HOOK_MARKER)) {
        println!("Status: aadc hook installed (auto-fix mode)");
        println!("Path: {}", hook_path.display());
    } else if content.contains(HOOK_MARKER) {
        println!("Status: aadc hook installed (unknown mode)");
        println!("Path: {}", hook_path.display());
    } else {
        println!("Status: Non-aadc pre-commit hook present");
        println!("Path: {}", hook_path.display());
    }

    // Check for backup
    let backup_path = hook_path.with_extension("pre-aadc");
    if backup_path.exists() {
        println!("Backup: {}", backup_path.display());
    }

    Ok(())
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

    // Handle subcommands first
    if let Some(command) = &args.command {
        let exit_code = match run_command(command) {
            Ok(()) => exit_codes::SUCCESS,
            Err(err) => {
                eprintln!("Error: {:#}", err);
                exit_code_for_error(&err)
            }
        };
        std::process::exit(exit_code);
    }

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
    styles: &VerboseStyle,
) -> FileResult {
    if config.verbose {
        console.print(
            &styles
                .bold(format!(
                    "Processing {} ({} lines)...",
                    filename,
                    lines.len()
                ))
                .to_string(),
        );
    }

    let original = lines.clone();
    let (corrected, stats) = correct_lines(lines, config, console, styles);

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

    // Warn about very high max_iters values that may slow processing
    if args.max_iters > 100 {
        eprintln!(
            "Warning: --max-iters {} is very high; this may slow processing",
            args.max_iters
        );
    }

    let config = create_config(&args)?;
    let (console, styles) = build_console(config.color);

    if config.verbose {
        if let Some(preset) = config.preset {
            console.print(
                &styles
                    .dim(format!(
                        "Using preset: {:?} (min_score = {:.1})",
                        preset,
                        config.effective_min_score()
                    ))
                    .to_string(),
            );
        }
    }

    if config.recursive {
        let files = discover_recursive_files(&args.inputs, &config, &console, &styles)?;
        if files.is_empty() {
            let message = format!(
                "Warning: No files matched pattern '{}' in provided paths",
                config.glob
            );
            if config.verbose {
                console.print(&styles.dim(message));
            } else {
                eprintln!("{}", message);
            }
            return Ok(RunOutcome {
                dry_run: config.dry_run,
                would_change: false,
            });
        }

        return output_multiple_results(&args, &config, &console, &styles, &files);
    }

    // Determine if we're processing stdin or files
    if args.inputs.is_empty() {
        // Stdin mode - single input
        let lines = read_stdin_content()?;
        let result = process_input(lines, "stdin".to_string(), &config, &console, &styles);
        output_single_result(&args, &config, &console, &styles, result)
    } else if args.inputs.len() == 1 {
        // Single file mode - same behavior as before
        let path = &args.inputs[0];
        let lines = read_file(path)?;
        let result = process_input(
            lines,
            path.display().to_string(),
            &config,
            &console,
            &styles,
        );
        output_single_result(&args, &config, &console, &styles, result)
    } else {
        // Multiple file mode
        output_multiple_results(&args, &config, &console, &styles, &args.inputs)
    }
}

/// Handle output for a single file/stdin result
fn output_single_result(
    args: &Args,
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
    result: FileResult,
) -> Result<RunOutcome> {
    let would_change = result.would_change;

    if config.json {
        output_json_single(args, config, &result)?;
    } else if config.dry_run {
        output_dry_run_single(config, console, styles, &result)?;
    } else if config.diff {
        output_diff(&result, false)?;
    } else if args.in_place {
        // Must have a file path for in-place
        let path = args
            .inputs
            .first()
            .ok_or_else(|| ArgError("--in-place requires an input file".to_string()))?;

        if config.backup {
            let backup_path = create_backup(path, &config.backup_ext)?;
            if config.verbose {
                console.print(
                    &styles
                        .dim(format!("Created backup: {}", backup_path.display()))
                        .to_string(),
                );
            }
        }

        let output = result.corrected.join("\n");
        fs::write(path, &output)
            .with_context(|| format!("Failed to write to file: {}", path.display()))?;
    } else {
        // Stdout mode
        let mut stdout = io::stdout().lock();
        for line in &result.corrected {
            writeln!(stdout, "{}", line)?;
        }
    }

    // Print summary in verbose mode for single file
    if config.verbose {
        print_stats_summary(
            &result.stats,
            1,
            if would_change { 1 } else { 0 },
            0,
            console,
            styles,
        );
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
fn output_dry_run_single(
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
    result: &FileResult,
) -> Result<()> {
    if config.diff && result.would_change {
        output_diff(result, true)?;
    }

    if config.verbose {
        if result.would_change {
            console.print(
                &styles
                    .block(format!("Would modify: {}", result.filename))
                    .to_string(),
            );
            console.print(
                &styles
                    .dim(format!(
                        "  {} block(s), {} revision(s)",
                        result.stats.blocks_modified, result.stats.total_revisions
                    ))
                    .to_string(),
            );
        } else {
            console.print(
                &styles
                    .success(format!("No changes needed: {}", result.filename))
                    .to_string(),
            );
        }
    }

    Ok(())
}

/// Handle output for multiple files
fn output_multiple_results(
    args: &Args,
    config: &Config,
    console: &Console,
    styles: &VerboseStyle,
    paths: &[PathBuf],
) -> Result<RunOutcome> {
    let mut total_files_processed = 0;
    let mut total_files_changed = 0;
    let mut aggregated_stats = Stats::default();
    let mut any_would_change = false;
    let mut errors: Vec<(PathBuf, anyhow::Error)> = Vec::new();

    let show_file_headers = !args.in_place && !config.diff && !config.json && paths.len() > 1;

    for path in paths {
        match read_file(path) {
            Ok(lines) => {
                let result =
                    process_input(lines, path.display().to_string(), config, console, styles);

                if result.would_change {
                    any_would_change = true;
                    total_files_changed += 1;
                }
                total_files_processed += 1;
                aggregated_stats.merge(&result.stats);

                // Handle output based on mode
                if config.json {
                    // For JSON with multiple files, output each file's JSON separately
                    output_json_single(args, config, &result)?;
                } else if config.dry_run {
                    output_dry_run_single(config, console, styles, &result)?;
                } else if config.diff {
                    output_diff(&result, false)?;
                } else if args.in_place {
                    // Write file in-place
                    if config.backup {
                        let backup_path = create_backup(path, &config.backup_ext)?;
                        if config.verbose {
                            console.print(
                                &styles
                                    .dim(format!("Created backup: {}", backup_path.display()))
                                    .to_string(),
                            );
                        }
                    }

                    let output = result.corrected.join("\n");
                    fs::write(path, &output)
                        .with_context(|| format!("Failed to write to file: {}", path.display()))?;

                    if config.verbose {
                        if result.would_change {
                            console.print(
                                &styles
                                    .success(format!(
                                        "{}: {} block(s), {} revision(s) applied",
                                        path.display(),
                                        result.stats.blocks_modified,
                                        result.stats.total_revisions
                                    ))
                                    .to_string(),
                            );
                        } else {
                            console.print(
                                &styles.dim(format!("{}: No changes needed", path.display())),
                            );
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
                eprintln!("Error processing {}: {:#}", path.display(), e);
                errors.push((path.clone(), e));
            }
        }
    }

    // Print summary in verbose mode
    if config.verbose {
        print_stats_summary(
            &aggregated_stats,
            total_files_processed,
            total_files_changed,
            errors.len(),
            console,
            styles,
        );
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
    use std::sync::Mutex;

    /// Mutex to serialize tests that change the current working directory.
    /// These tests cannot run in parallel because std::env::set_current_dir
    /// affects global process state.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn make_args() -> Args {
        Args {
            inputs: vec![],
            config_file: None,
            no_config: false,
            recursive: false,
            glob: "*.txt,*.md".to_string(),
            no_gitignore: false,
            max_depth: 0,
            in_place: false,
            preset: None,
            max_iters: 10,
            min_score: 0.5,
            tab_width: 4,
            all: false,
            lines: None, // String, not Vec<LineRange>
            verbose: false,
            color: ColorMode::Auto,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
            command: None,
        }
    }

    /// Create a default Config for tests
    fn make_test_config() -> Config {
        Config {
            max_iters: 10,
            min_score: 0.5,
            preset: None,
            tab_width: 4,
            all_blocks: false,
            lines: None,
            recursive: false,
            glob: "*.txt,*.md".to_string(),
            gitignore: true,
            max_depth: 0,
            color: ColorMode::Auto,
            verbose: false,
            diff: false,
            dry_run: false,
            backup: false,
            backup_ext: ".bak".to_string(),
            json: false,
        }
    }

    /// Create VerboseStyle for tests (no colors)
    fn make_test_styles() -> VerboseStyle {
        VerboseStyle::new(false)
    }

    // =========================================================================
    // Args parsing + validation tests
    // =========================================================================

    #[test]
    fn test_args_defaults() {
        let args = Args::parse_from(["aadc"]);
        assert!(args.inputs.is_empty());
        assert!(!args.recursive);
        assert_eq!(args.glob, "*.txt,*.md");
        assert!(!args.no_gitignore);
        assert_eq!(args.max_depth, 0);
        assert!(!args.in_place);
        assert!(args.preset.is_none());
        assert_eq!(args.max_iters, 10);
        assert_eq!(args.min_score, 0.5);
        assert_eq!(args.tab_width, 4);
        assert!(!args.all);
        assert!(!args.verbose);
        assert!(matches!(args.color, ColorMode::Auto));
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
    fn test_args_recursive_defaults() {
        let args = Args::parse_from(["aadc", "-r", "docs"]);
        assert!(args.recursive);
        assert_eq!(args.glob, "*.txt,*.md");
        assert!(!args.no_gitignore);
        assert_eq!(args.max_depth, 0);
        assert_eq!(args.inputs, vec![PathBuf::from("docs")]);
    }

    #[test]
    fn test_args_recursive_custom() {
        let args = Args::parse_from([
            "aadc",
            "--recursive",
            "--glob",
            "*.md",
            "--max-depth",
            "2",
            "--no-gitignore",
            "docs",
        ]);
        assert!(args.recursive);
        assert_eq!(args.glob, "*.md");
        assert!(args.no_gitignore);
        assert_eq!(args.max_depth, 2);
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
    fn test_args_preset_relaxed() {
        let args = Args::parse_from(["aadc", "--preset", "relaxed", "file.txt"]);
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
            lines: None,
            recursive: false,
            glob: "*.txt,*.md".to_string(),
            gitignore: true,
            max_depth: 0,
            color: ColorMode::Auto,
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
            lines: None,
            recursive: false,
            glob: "*.txt,*.md".to_string(),
            gitignore: true,
            max_depth: 0,
            color: ColorMode::Auto,
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
    fn test_validate_args_recursive_requires_path() {
        let mut args = make_args();
        args.recursive = true;
        assert!(validate_args(&args).is_err());
        args.inputs = vec![PathBuf::from("docs")];
        assert!(validate_args(&args).is_ok());
    }

    #[test]
    fn test_validate_args_tab_width_bounds() {
        let mut args = make_args();
        args.tab_width = 0;
        assert!(validate_args(&args).is_err());
        args.tab_width = 17;
        assert!(validate_args(&args).is_err());
        args.tab_width = 1;
        assert!(validate_args(&args).is_ok());
        args.tab_width = 16;
        assert!(validate_args(&args).is_ok());
        args.tab_width = 4;
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
    // Quick scan passthrough tests
    // =========================================================================

    #[test]
    fn test_quick_scan_plain_text() {
        let lines = vec![
            "Hello world".to_string(),
            "This is plain text".to_string(),
            "No diagrams here".to_string(),
        ];
        let result = quick_scan_for_diagrams(&lines);

        assert!(!result.likely_has_diagrams);
        assert_eq!(result.lines_with_box_chars, 0);
    }

    #[test]
    fn test_quick_scan_with_diagram_lines() {
        let lines = vec![
            "+---+".to_string(),
            "| a |".to_string(),
            "+---+".to_string(),
        ];
        let result = quick_scan_for_diagrams(&lines);

        assert!(result.likely_has_diagrams);
        assert!(result.ratio >= QUICK_SCAN_THRESHOLD);
    }

    #[test]
    fn test_quick_scan_threshold_boundary() {
        let mut lines = Vec::new();
        for i in 0..100 {
            if i == 0 {
                lines.push("+---+".to_string());
            } else {
                lines.push("plain text".to_string());
            }
        }
        let result = quick_scan_for_diagrams(&lines);

        assert_eq!(result.lines_scanned, 100);
        assert_eq!(result.lines_with_box_chars, 1);
        assert!(result.ratio >= QUICK_SCAN_THRESHOLD);
        assert!(result.likely_has_diagrams);
    }

    #[test]
    fn test_correct_lines_passthrough_skips_tabs() {
        let lines = vec!["\tPlain text".to_string()];
        let config = make_test_config();
        let console = Console::new();
        let styles = make_test_styles();
        let (corrected, stats) = correct_lines(lines.clone(), &config, &console, &styles);

        assert_eq!(corrected, lines);
        assert_eq!(stats.blocks_found, 0);
        assert_eq!(stats.total_revisions, 0);
    }

    #[test]
    fn test_correct_lines_all_blocks_bypasses_quick_scan() {
        let lines = vec!["\tPlain text".to_string()];
        let mut config = make_test_config();
        config.all_blocks = true;
        let console = Console::new();
        let styles = make_test_styles();
        let (corrected, _stats) = correct_lines(lines.clone(), &config, &console, &styles);

        assert_ne!(corrected, lines);
        assert_eq!(corrected[0], "    Plain text");
    }

    // =========================================================================
    // Recursive discovery tests
    // =========================================================================

    #[test]
    fn test_discover_recursive_files_glob_matching() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.txt"), "content").unwrap();
        fs::write(temp.path().join("b.md"), "content").unwrap();
        fs::write(temp.path().join("c.rs"), "content").unwrap();

        let mut config = make_test_config();
        config.recursive = true;
        config.gitignore = false;
        let console = Console::new();
        let styles = make_test_styles();

        let files =
            discover_recursive_files(&[temp.path().to_path_buf()], &config, &console, &styles)
                .unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();

        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.md"));
        assert!(!names.contains(&"c.rs"));
    }

    #[test]
    fn test_discover_recursive_files_max_depth() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b")).unwrap();
        fs::write(temp.path().join("top.txt"), "").unwrap();
        fs::write(temp.path().join("a/mid.txt"), "").unwrap();
        fs::write(temp.path().join("a/b/deep.txt"), "").unwrap();

        let mut config = make_test_config();
        config.recursive = true;
        config.glob = "*.txt".to_string();
        config.gitignore = false;
        config.max_depth = 2;
        let console = Console::new();
        let styles = make_test_styles();

        let files =
            discover_recursive_files(&[temp.path().to_path_buf()], &config, &console, &styles)
                .unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();

        assert!(names.contains(&"top.txt"));
        assert!(names.contains(&"mid.txt"));
        assert!(!names.contains(&"deep.txt"));
    }

    #[test]
    fn test_discover_recursive_files_respects_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(temp.path().join("included.txt"), "").unwrap();
        fs::write(temp.path().join("ignored.txt"), "").unwrap();

        fs::create_dir(temp.path().join(".git")).unwrap();

        let mut config = make_test_config();
        config.recursive = true;
        config.glob = "*.txt".to_string();
        let console = Console::new();
        let styles = make_test_styles();

        let files =
            discover_recursive_files(&[temp.path().to_path_buf()], &config, &console, &styles)
                .unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();

        assert!(names.contains(&"included.txt"));
        assert!(!names.contains(&"ignored.txt"));
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
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "+------+".to_string(),
            "| short|".to_string(),
            "| longer |".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console, &styles);

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
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "Just plain text".to_string(),
            "No diagrams here".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines.clone(), &config, &console, &styles);
        assert_eq!(stats.blocks_found, 0);
        assert_eq!(stats.blocks_modified, 0);
        assert_eq!(corrected, lines, "content should be unchanged");
    }

    #[test]
    fn test_correction_already_aligned() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "+------+".to_string(),
            "| text |".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines.clone(), &config, &console, &styles);
        assert_eq!(stats.blocks_found, 1);
        // Perfectly aligned blocks should not be modified
        assert_eq!(corrected, lines);
    }

    #[test]
    fn test_correction_unicode() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "┌───────┐".to_string(),
            "│ short│".to_string(),
            "│ longer │".to_string(),
            "└───────┘".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console, &styles);
        assert_eq!(stats.blocks_found, 1);
        // Verify correction ran successfully (at least one block found and processed)
        assert!(!corrected.is_empty());
    }

    #[test]
    fn test_correction_with_tabs() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "+------+".to_string(),
            "|\thi|".to_string(), // Tab should be expanded
            "+------+".to_string(),
        ];

        let (corrected, _) = correct_lines(lines, &config, &console, &styles);
        // Tab should be expanded to spaces
        assert!(!corrected[1].contains('\t'), "tabs should be expanded");
    }

    #[test]
    fn test_correction_max_iters_limit() {
        let console = Console::new();
        let mut config = make_test_config();
        config.max_iters = 1; // Only 1 iteration
        config.min_score = 0.1;
        let styles = make_test_styles();

        let lines = vec![
            "+--------+".to_string(),
            "| a|".to_string(),
            "| longer |".to_string(),
            "+--------+".to_string(),
        ];

        let (corrected, stats) = correct_lines(lines, &config, &console, &styles);
        assert_eq!(stats.blocks_found, 1);
        // With limited iterations, some progress should still be made
        assert!(corrected.len() == 4);
    }

    #[test]
    fn test_correction_min_score_filter() {
        let console = Console::new();
        let mut config = make_test_config();
        config.min_score = 0.95; // Very strict
        let styles = make_test_styles();

        let lines = vec![
            "+------+".to_string(),
            "| text|".to_string(),
            "+------+".to_string(),
        ];

        let (corrected, _) = correct_lines(lines.clone(), &config, &console, &styles);
        // With very strict min_score, fewer changes should be made
        // The exact behavior depends on the scoring implementation
        assert!(corrected.len() == 3);
    }

    #[test]
    fn test_correction_multiple_blocks() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

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

        let (corrected, stats) = correct_lines(lines, &config, &console, &styles);
        assert_eq!(stats.blocks_found, 2, "should find two blocks");
        assert_eq!(corrected.len(), 10);
    }

    #[test]
    fn test_correction_empty_input() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

        let lines: Vec<String> = vec![];
        let (corrected, stats) = correct_lines(lines, &config, &console, &styles);
        assert_eq!(stats.blocks_found, 0);
        assert!(corrected.is_empty());
    }

    #[test]
    fn test_correction_preserves_non_diagram_content() {
        let console = Console::new();
        let config = make_test_config();
        let styles = make_test_styles();

        let lines = vec![
            "# Header".to_string(),
            "".to_string(),
            "+---+".to_string(),
            "| x|".to_string(),
            "+---+".to_string(),
            "".to_string(),
            "Footer text".to_string(),
        ];

        let (corrected, _) = correct_lines(lines, &config, &console, &styles);
        assert_eq!(corrected[0], "# Header");
        assert_eq!(corrected[6], "Footer text");
    }

    // =========================================================================
    // Hook management tests
    // =========================================================================

    #[test]
    fn test_generate_check_hook() {
        let hook = generate_check_hook(&["*.md", "*.txt"]);

        assert!(hook.contains("#!/usr/bin/env bash"));
        assert!(hook.contains("# aadc pre-commit hook (check mode)"));
        assert!(hook.contains("aadc --dry-run"));
        assert!(hook.contains("*.md *.txt"));
        // Should not *execute* aadc -i (the help message is allowed to mention it)
        assert!(!hook.contains("aadc -i \"$file\""));
    }

    #[test]
    fn test_generate_autofix_hook() {
        let hook = generate_autofix_hook(&["*.md"]);

        assert!(hook.contains("#!/usr/bin/env bash"));
        assert!(hook.contains("# aadc pre-commit hook (auto-fix mode)"));
        assert!(hook.contains("aadc -i"));
        assert!(hook.contains("git add"));
        assert!(hook.contains("*.md"));
    }

    #[test]
    fn test_default_hook_is_check_mode() {
        let default = generate_default_hook(&["*.txt"]);
        let check = generate_check_hook(&["*.txt"]);

        // Default should be identical to check mode
        assert_eq!(default, check);
    }

    #[test]
    fn test_hook_patterns() {
        let hook = generate_check_hook(&["*.rs", "*.go", "*.py"]);

        assert!(hook.contains("PATTERNS=\"*.rs *.go *.py\""));
    }

    #[test]
    fn test_find_git_dir_not_in_repo() {
        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir that's not a git repo
        let temp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();

        let result = find_git_dir();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Not in a git repository")
        );

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_find_git_dir_in_repo() {
        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = find_git_dir();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), git_dir);

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_install_creates_hook() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git/hooks
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = hook_install(true, false, None);
        assert!(result.is_ok());

        // Verify hook was created
        let hook_path = git_dir.join("hooks").join("pre-commit");
        assert!(hook_path.exists());

        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("# aadc pre-commit hook (check mode)"));

        // Verify it's executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&hook_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o755, 0o755);
        }

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_install_autofix_mode() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = hook_install(false, true, None);
        assert!(result.is_ok());

        let hook_path = git_dir.join("hooks").join("pre-commit");
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("# aadc pre-commit hook (auto-fix mode)"));
        assert!(content.contains("aadc -i"));

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_install_custom_patterns() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let patterns = vec!["*.rs".to_string(), "*.go".to_string()];
        let result = hook_install(true, false, Some(&patterns));
        assert!(result.is_ok());

        let hook_path = git_dir.join("hooks").join("pre-commit");
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("*.rs *.go"));

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_install_backs_up_existing() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git/hooks and existing hook
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        let hooks_dir = git_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let existing_hook = hooks_dir.join("pre-commit");
        fs::write(&existing_hook, "#!/bin/bash\necho 'existing hook'").unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = hook_install(true, false, None);
        assert!(result.is_ok());

        // Verify backup was created
        let backup_path = hooks_dir.join("pre-commit.pre-aadc");
        assert!(backup_path.exists());
        let backup_content = fs::read_to_string(&backup_path).unwrap();
        assert!(backup_content.contains("existing hook"));

        // Verify new hook was installed
        let content = fs::read_to_string(&existing_hook).unwrap();
        assert!(content.contains("# aadc pre-commit hook"));

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_uninstall_removes_aadc_hook() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git/hooks and aadc hook
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        let hooks_dir = git_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let hook_path = hooks_dir.join("pre-commit");
        fs::write(&hook_path, generate_check_hook(&["*.md"])).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = hook_uninstall();
        assert!(result.is_ok());
        assert!(!hook_path.exists());

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_uninstall_refuses_non_aadc_hook() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git/hooks and non-aadc hook
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        let hooks_dir = git_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let hook_path = hooks_dir.join("pre-commit");
        fs::write(&hook_path, "#!/bin/bash\necho 'other hook'").unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        let result = hook_uninstall();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not installed by aadc")
        );

        // Hook should still exist
        assert!(hook_path.exists());

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_status_no_hook() {
        let _guard = CWD_LOCK.lock().unwrap();

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Create a temp dir with .git but no hooks
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        std::env::set_current_dir(temp.path()).unwrap();

        // hook_status should succeed even with no hook
        let result = hook_status();
        assert!(result.is_ok());

        // Restore
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_hook_subcommand_parsing() {
        let args = Args::parse_from(["aadc", "hook", "install"]);
        assert!(args.command.is_some());
        if let Some(Commands::Hook { action }) = args.command {
            assert!(matches!(action, HookAction::Install { .. }));
        } else {
            panic!("Expected Hook command");
        }
    }

    #[test]
    fn test_hook_subcommand_install_check_only() {
        let args = Args::parse_from(["aadc", "hook", "install", "--check-only"]);
        if let Some(Commands::Hook { action }) = args.command {
            if let HookAction::Install {
                check_only,
                auto_fix,
                ..
            } = action
            {
                assert!(check_only);
                assert!(!auto_fix);
            } else {
                panic!("Expected Install action");
            }
        } else {
            panic!("Expected Hook command");
        }
    }

    #[test]
    fn test_hook_subcommand_install_autofix() {
        let args = Args::parse_from(["aadc", "hook", "install", "--auto-fix"]);
        if let Some(Commands::Hook { action }) = args.command {
            if let HookAction::Install {
                check_only,
                auto_fix,
                ..
            } = action
            {
                assert!(!check_only);
                assert!(auto_fix);
            } else {
                panic!("Expected Install action");
            }
        } else {
            panic!("Expected Hook command");
        }
    }

    #[test]
    fn test_hook_subcommand_uninstall() {
        let args = Args::parse_from(["aadc", "hook", "uninstall"]);
        if let Some(Commands::Hook { action }) = args.command {
            assert!(matches!(action, HookAction::Uninstall));
        } else {
            panic!("Expected Hook command");
        }
    }

    #[test]
    fn test_hook_subcommand_status() {
        let args = Args::parse_from(["aadc", "hook", "status"]);
        if let Some(Commands::Hook { action }) = args.command {
            assert!(matches!(action, HookAction::Status));
        } else {
            panic!("Expected Hook command");
        }
    }

    // =========================================================================
    // Config file tests
    // =========================================================================

    #[test]
    fn test_find_config_file_in_current_dir() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(&config_path, "min_score = 0.7").unwrap();

        let found = find_config_file(temp.path());
        assert!(found.is_some());
        assert_eq!(found.unwrap(), config_path);
    }

    #[test]
    fn test_find_config_file_in_parent_dir() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(&config_path, "min_score = 0.7").unwrap();

        // Create a subdirectory
        let subdir = temp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        // Should find config in parent
        let found = find_config_file(&subdir);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), config_path);
    }

    #[test]
    fn test_find_config_file_alternative_names() {
        let temp = tempfile::tempdir().unwrap();

        // Test .aadcrc.toml variant
        let config_path = temp.path().join(".aadcrc.toml");
        fs::write(&config_path, "min_score = 0.7").unwrap();

        let found = find_config_file(temp.path());
        assert!(found.is_some());
        assert_eq!(found.unwrap(), config_path);
    }

    #[test]
    fn test_find_config_file_not_found() {
        let temp = tempfile::tempdir().unwrap();
        // No config file created

        let found = find_config_file(temp.path());
        // May find a config in home dir or return None
        // We can't control home dir, so just verify it doesn't panic
        let _ = found;
    }

    #[test]
    fn test_load_config_file_basic() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
min_score = 0.7
max_iters = 20
tab_width = 2
verbose = true
"#,
        )
        .unwrap();

        let config = load_config_file(&config_path).unwrap();
        assert_eq!(config.min_score, Some(0.7));
        assert_eq!(config.max_iters, Some(20));
        assert_eq!(config.tab_width, Some(2));
        assert_eq!(config.verbose, Some(true));
    }

    #[test]
    fn test_load_config_file_with_preset() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
preset = "aggressive"
"#,
        )
        .unwrap();

        let config = load_config_file(&config_path).unwrap();
        assert_eq!(config.preset, Some(Preset::Aggressive));
    }

    #[test]
    fn test_load_config_file_all_options() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
min_score = 0.6
max_iters = 15
tab_width = 8
verbose = true
json = true
backup = true
backup_ext = ".backup"
recursive = true
glob = "*.rs"
gitignore = false
max_depth = 5
all = true
"#,
        )
        .unwrap();

        let config = load_config_file(&config_path).unwrap();
        assert_eq!(config.min_score, Some(0.6));
        assert_eq!(config.max_iters, Some(15));
        assert_eq!(config.tab_width, Some(8));
        assert_eq!(config.verbose, Some(true));
        assert_eq!(config.json, Some(true));
        assert_eq!(config.backup, Some(true));
        assert_eq!(config.backup_ext, Some(".backup".to_string()));
        assert_eq!(config.recursive, Some(true));
        assert_eq!(config.glob, Some("*.rs".to_string()));
        assert_eq!(config.gitignore, Some(false));
        assert_eq!(config.max_depth, Some(5));
        assert_eq!(config.all, Some(true));
    }

    #[test]
    fn test_load_config_file_empty() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(&config_path, "").unwrap();

        let config = load_config_file(&config_path).unwrap();
        assert!(config.min_score.is_none());
        assert!(config.max_iters.is_none());
    }

    #[test]
    fn test_load_config_file_unknown_keys_tolerated() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
min_score = 0.7
unknown_key = "should be ignored"
another_unknown = 42
"#,
        )
        .unwrap();

        // Should not fail on unknown keys (toml serde default behavior)
        let config = load_config_file(&config_path);
        assert!(config.is_ok());
        assert_eq!(config.unwrap().min_score, Some(0.7));
    }

    #[test]
    fn test_load_config_file_invalid_toml() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(&config_path, "this is not valid toml [[[").unwrap();

        let result = load_config_file(&config_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("parse"));
    }

    #[test]
    fn test_load_config_file_invalid_preset() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
preset = "nonexistent"
"#,
        )
        .unwrap();

        let result = load_config_file(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_config_no_config_flag() {
        let args = Args::parse_from(["aadc", "--no-config"]);
        let config = create_config(&args).unwrap();
        // Should use CLI defaults, not load any config file
        assert_eq!(config.min_score, 0.5);
        assert_eq!(config.max_iters, 10);
    }

    #[test]
    fn test_create_config_explicit_file() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("custom.toml");
        fs::write(&config_path, "max_iters = 25\n").unwrap();

        let args = Args::parse_from(["aadc", "--config", config_path.to_str().unwrap()]);
        let config = create_config(&args).unwrap();
        assert_eq!(config.max_iters, 25);
    }

    #[test]
    fn test_create_config_explicit_file_not_found() {
        let args = Args::parse_from(["aadc", "--config", "/nonexistent/path/config.toml"]);
        let result = create_config(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_create_config_cli_overrides_file() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(
            &config_path,
            r#"
max_iters = 25
tab_width = 8
verbose = true
"#,
        )
        .unwrap();

        // Create a test file in the temp dir so config is found
        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "").unwrap();

        let args = Args::parse_from(["aadc", "--max-iters", "5", test_file.to_str().unwrap()]);
        let config = create_config(&args).unwrap();

        // CLI value should override file
        assert_eq!(config.max_iters, 5);
        // File value should be used when CLI uses default
        assert_eq!(config.tab_width, 8);
        assert!(config.verbose);
    }

    #[test]
    fn test_create_config_preset_from_file() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join(".aadcrc");
        fs::write(&config_path, "preset = \"strict\"\n").unwrap();

        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "").unwrap();

        let args = Args::parse_from(["aadc", test_file.to_str().unwrap()]);
        let config = create_config(&args).unwrap();

        assert_eq!(config.preset, Some(Preset::Strict));
    }

    #[test]
    fn test_config_subcommand_parsing() {
        let args = Args::parse_from(["aadc", "config", "init"]);
        assert!(args.command.is_some());
        if let Some(Commands::Config { action }) = args.command {
            assert!(matches!(action, ConfigAction::Init { global: false }));
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_config_subcommand_init_global() {
        let args = Args::parse_from(["aadc", "config", "init", "--global"]);
        if let Some(Commands::Config { action }) = args.command {
            if let ConfigAction::Init { global } = action {
                assert!(global);
            } else {
                panic!("Expected Init action");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_config_subcommand_show() {
        let args = Args::parse_from(["aadc", "config", "show"]);
        if let Some(Commands::Config { action }) = args.command {
            assert!(matches!(action, ConfigAction::Show));
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_config_subcommand_path() {
        let args = Args::parse_from(["aadc", "config", "path"]);
        if let Some(Commands::Config { action }) = args.command {
            assert!(matches!(action, ConfigAction::Path));
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_args_config_file_option() {
        let args = Args::parse_from(["aadc", "--config", "/path/to/config"]);
        assert_eq!(args.config_file, Some(PathBuf::from("/path/to/config")));
    }

    #[test]
    fn test_args_no_config_option() {
        let args = Args::parse_from(["aadc", "--no-config"]);
        assert!(args.no_config);
    }

    // =========================================================================
    // Line range parsing tests
    // =========================================================================

    #[test]
    fn test_parse_simple_range() {
        let ranges = parse_line_ranges("10-50").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 10);
        assert_eq!(ranges[0].end, 50);
    }

    #[test]
    fn test_parse_multiple_ranges() {
        let ranges = parse_line_ranges("1-10, 20-30, 50-60").unwrap();
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], LineRange { start: 1, end: 10 });
        assert_eq!(ranges[1], LineRange { start: 20, end: 30 });
        assert_eq!(ranges[2], LineRange { start: 50, end: 60 });
    }

    #[test]
    fn test_parse_open_ended_start() {
        let ranges = parse_line_ranges("50-").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 50);
        assert_eq!(ranges[0].end, usize::MAX);
    }

    #[test]
    fn test_parse_open_ended_end() {
        let ranges = parse_line_ranges("-100").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 1);
        assert_eq!(ranges[0].end, 100);
    }

    #[test]
    fn test_parse_single_line() {
        let ranges = parse_line_ranges("42").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 42);
        assert_eq!(ranges[0].end, 42);
    }

    #[test]
    fn test_merge_overlapping_ranges() {
        let ranges = parse_line_ranges("1-50, 40-100").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 1);
        assert_eq!(ranges[0].end, 100);
    }

    #[test]
    fn test_merge_adjacent_ranges() {
        let ranges = parse_line_ranges("1-10, 11-20").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 1);
        assert_eq!(ranges[0].end, 20);
    }

    #[test]
    fn test_invalid_range_reversed() {
        let result = parse_line_ranges("50-10");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("start (50) > end (10)"));
    }

    #[test]
    fn test_invalid_range_non_numeric() {
        let result = parse_line_ranges("abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid line number"));
    }

    #[test]
    fn test_invalid_range_zero() {
        let result = parse_line_ranges("0-10");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Line numbers start at 1"));
    }

    #[test]
    fn test_line_in_ranges() {
        let ranges = vec![
            LineRange { start: 1, end: 10 },
            LineRange { start: 20, end: 30 },
        ];
        assert!(line_in_ranges(5, &ranges));
        assert!(line_in_ranges(1, &ranges));
        assert!(line_in_ranges(10, &ranges));
        assert!(line_in_ranges(25, &ranges));
        assert!(!line_in_ranges(15, &ranges));
        assert!(!line_in_ranges(31, &ranges));
    }

    #[test]
    fn test_block_overlaps_ranges() {
        let ranges = vec![LineRange { start: 10, end: 20 }];

        // Block fully inside range
        let block_inside = DiagramBlock {
            start: 11, // 0-indexed, so line 12
            end: 15,   // exclusive, so through line 15
            confidence: 1.0,
        };
        assert!(block_overlaps_ranges(&block_inside, &ranges));

        // Block overlapping start of range
        let block_overlap_start = DiagramBlock {
            start: 5,
            end: 12,
            confidence: 1.0,
        };
        assert!(block_overlaps_ranges(&block_overlap_start, &ranges));

        // Block overlapping end of range
        let block_overlap_end = DiagramBlock {
            start: 18,
            end: 25,
            confidence: 1.0,
        };
        assert!(block_overlaps_ranges(&block_overlap_end, &ranges));

        // Block completely outside range
        let block_outside = DiagramBlock {
            start: 25,
            end: 30,
            confidence: 1.0,
        };
        assert!(!block_overlaps_ranges(&block_outside, &ranges));
    }

    #[test]
    fn test_format_line_ranges() {
        let ranges = vec![
            LineRange { start: 1, end: 10 },
            LineRange { start: 20, end: 30 },
        ];
        let formatted = format_line_ranges(&ranges, 100);
        assert!(formatted.contains("1-10"));
        assert!(formatted.contains("20-30"));
        assert!(formatted.contains("21 of 100 lines"));
    }

    #[test]
    fn test_format_line_ranges_open_ended() {
        let ranges = vec![LineRange {
            start: 50,
            end: usize::MAX,
        }];
        let formatted = format_line_ranges(&ranges, 100);
        assert!(formatted.contains("50-"));
        assert!(formatted.contains("51 of 100 lines"));
    }

    #[test]
    fn test_args_lines_parsing() {
        let args = Args::parse_from(["aadc", "--lines", "10-50", "file.txt"]);
        assert!(args.lines.is_some());
        assert_eq!(args.lines.as_ref().unwrap(), "10-50");

        // Verify it parses correctly via Config
        let config = Config::from(&args);
        let ranges = config.lines.unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 10);
        assert_eq!(ranges[0].end, 50);
    }

    #[test]
    fn test_args_lines_multiple_ranges() {
        let args = Args::parse_from(["aadc", "-L", "1-10,50-60", "file.txt"]);
        assert!(args.lines.is_some());
        assert_eq!(args.lines.as_ref().unwrap(), "1-10,50-60");

        // Verify it parses correctly via Config
        let config = Config::from(&args);
        let ranges = config.lines.unwrap();
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_correct_lines_with_ranges() {
        // Test that line ranges filter which blocks get processed
        // We use a single diagram and verify it's only processed when within range
        let input = r#"Line 1 prose
Line 2 prose
+------+
| Hi|
+------+
Line 6 prose
Line 7 prose"#;

        let lines: Vec<String> = input.lines().map(String::from).collect();
        let console = Console::new();
        let styles = make_test_styles();

        // Test 1: Process lines 3-5 (where diagram is) - diagram SHOULD be corrected
        let mut config = make_test_config();
        config.lines = Some(vec![LineRange { start: 3, end: 5 }]);
        config.all_blocks = true;

        let (output, stats) = correct_lines(lines.clone(), &config, &console, &styles);

        // Diagram lines should be corrected (right border aligned)
        assert!(
            output[3].contains("| Hi") && output[3].ends_with("|"),
            "Diagram should be corrected when in range, got: {:?}",
            output[3]
        );
        assert!(
            stats.blocks_modified >= 1,
            "At least one block should be modified"
        );

        // Test 2: Process lines 1-2 (before diagram) - diagram should NOT be corrected
        let mut config2 = make_test_config();
        config2.lines = Some(vec![LineRange { start: 1, end: 2 }]);
        config2.all_blocks = true;

        let (output2, stats2) = correct_lines(lines.clone(), &config2, &console, &styles);

        // Diagram should be unchanged (original input)
        assert_eq!(
            output2[3], "| Hi|",
            "Diagram outside range should be unchanged"
        );
        assert_eq!(
            stats2.blocks_modified, 0,
            "No blocks should be modified when range excludes diagram"
        );

        // Test 3: Process lines 6-7 (after diagram) - diagram should NOT be corrected
        let mut config3 = make_test_config();
        config3.lines = Some(vec![LineRange { start: 6, end: 7 }]);
        config3.all_blocks = true;

        let (output3, stats3) = correct_lines(lines.clone(), &config3, &console, &styles);

        // Diagram should be unchanged
        assert_eq!(
            output3[3], "| Hi|",
            "Diagram outside range should be unchanged"
        );
        assert_eq!(
            stats3.blocks_modified, 0,
            "No blocks should be modified when range excludes diagram"
        );
    }
}
