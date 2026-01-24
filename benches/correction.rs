//! Criterion benchmarks for aadc performance testing.
//!
//! These benchmarks measure the performance of the aadc binary by invoking
//! it as a subprocess. This approach tests real-world performance including
//! process startup, file I/O, and the complete correction pipeline.
//!
//! For micro-benchmarks of internal functions, the code would need to be
//! refactored to expose a library interface.

use criterion::{Criterion, criterion_group, criterion_main};
use std::path::PathBuf;
use std::process::Command;

fn aadc_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_aadc") {
        return PathBuf::from(path);
    }

    let debug = PathBuf::from("target/debug/aadc");
    if debug.exists() {
        return debug;
    }

    let release = PathBuf::from("target/release/aadc");
    if release.exists() {
        return release;
    }

    panic!("aadc binary not found; set CARGO_BIN_EXE_aadc or build target/debug|release");
}

/// Benchmark processing a small ASCII diagram file
fn bench_small_file(c: &mut Criterion) {
    let input_file = "tests/fixtures/ascii/simple_box.input.txt";

    // Skip if file doesn't exist
    if !std::path::Path::new(input_file).exists() {
        eprintln!("Skipping bench_small_file: {} not found", input_file);
        return;
    }

    let aadc = aadc_binary();

    c.bench_function("small_file", |b| {
        b.iter(|| {
            Command::new(&aadc)
                .arg(input_file)
                .output()
                .expect("Failed to execute aadc")
        })
    });
}

/// Benchmark processing a medium-sized file (100 lines)
fn bench_medium_file(c: &mut Criterion) {
    let input_file = "tests/fixtures/large/100_lines.input.txt";

    if !std::path::Path::new(input_file).exists() {
        eprintln!("Skipping bench_medium_file: {} not found", input_file);
        return;
    }

    let aadc = aadc_binary();

    c.bench_function("medium_file", |b| {
        b.iter(|| {
            Command::new(&aadc)
                .arg(input_file)
                .output()
                .expect("Failed to execute aadc")
        })
    });
}

/// Benchmark processing CJK content (tests visual_width complexity)
fn bench_cjk_content(c: &mut Criterion) {
    let input_file = "tests/fixtures/large/cjk_content.input.txt";

    if !std::path::Path::new(input_file).exists() {
        eprintln!("Skipping bench_cjk_content: {} not found", input_file);
        return;
    }

    let aadc = aadc_binary();

    c.bench_function("cjk_content", |b| {
        b.iter(|| {
            Command::new(&aadc)
                .arg(input_file)
                .output()
                .expect("Failed to execute aadc")
        })
    });
}

/// Benchmark verbose mode (tests console output overhead)
fn bench_verbose_mode(c: &mut Criterion) {
    let input_file = "tests/fixtures/large/100_lines.input.txt";

    if !std::path::Path::new(input_file).exists() {
        eprintln!("Skipping bench_verbose_mode: {} not found", input_file);
        return;
    }

    let aadc = aadc_binary();

    c.bench_function("verbose_mode", |b| {
        b.iter(|| {
            Command::new(&aadc)
                .arg("-v")
                .arg(input_file)
                .output()
                .expect("Failed to execute aadc")
        })
    });
}

criterion_group!(
    benches,
    bench_small_file,
    bench_medium_file,
    bench_cjk_content,
    bench_verbose_mode
);
criterion_main!(benches);
