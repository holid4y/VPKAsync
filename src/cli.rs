use anyhow::{anyhow, Result};
use clap::Parser;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::packer::inspect::{self, VpkMode};
use crate::packer::{self, PackOptions};

#[derive(Parser)]
#[command(
    name = "async_vpk",
    about = "Async/parallel VPK creator for TF2 - replacement for Valve's vpk.exe",
    version
)]
struct Cli {
    #[arg(
        help = "Folder to be packaged into a VPK",
        required_unless_present_any = ["verify", "list"]
    )]
    input: Option<PathBuf>,

    #[arg(
        short,
        long,
        help = "Output base path (without extension). Default: input folder name"
    )]
    output: Option<PathBuf>,

    #[arg(
        long,
        conflicts_with = "multi",
        help = "Force single-file mode even above 600 MB"
    )]
    single: bool,

    #[arg(
        long,
        conflicts_with = "single",
        help = "Force multi-chunk mode even below 600 MB"
    )]
    multi: bool,

    #[arg(
        short,
        long,
        value_parser = clap::value_parser!(NonZeroUsize),
        help = "Number of threads (default: all available CPU cores)"
    )]
    threads: Option<NonZeroUsize>,

    #[arg(
        long,
        value_name = "VPK_FILE",
        conflicts_with_all = ["input", "output", "single", "multi", "threads", "list"],
        help = "Verify an existing VPK (single .vpk or _dir.vpk): parse the tree and CRC-check all data"
    )]
    verify: Option<PathBuf>,

    #[arg(
        long,
        value_name = "VPK_FILE",
        conflicts_with_all = ["input", "output", "single", "multi", "threads"],
        help = "List the files stored in an existing VPK (single .vpk or _dir.vpk)"
    )]
    list: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    if let Some(path) = cli.verify {
        return run_verify(&path);
    }
    if let Some(path) = cli.list {
        return run_list(&path);
    }

    let Some(input) = cli.input else {
        return Err(anyhow!("missing input folder"));
    };
    let output = cli
        .output
        .unwrap_or_else(|| packer::default_output_base(&input));

    let options = PackOptions {
        single: cli.single,
        multi: cli.multi,
        threads: cli.threads.map(|n| n.get()),
    };

    println!("===============================================");
    println!("  VPK Maker  |  Async/Parallel  |  TF2 compatible  ");
    println!("===============================================");

    let summary = packer::package_folder(&input, &output, options, |line| println!("{}", line))?;

    println!();
    println!("Done!");
    println!(
        "Mode used: {}",
        if summary.use_multi {
            "multi-chunk (_dir + _000.vpk...)"
        } else {
            "single-file (.vpk)"
        }
    );
    println!("Files skipped due to errors: {}", summary.skipped.len());
    for file in summary.generated_files {
        println!("  -> {}", file.display());
    }

    Ok(())
}

/// Paths and problem strings come from an untrusted archive; strip control
/// characters so a crafted VPK cannot inject terminal escape sequences.
fn sanitize_for_terminal(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn run_verify(path: &Path) -> Result<()> {
    let inspection = inspect::inspect_vpk(path, true)?;

    if !inspection.problems.is_empty() {
        println!(
            "FAILED: {} problem(s) in {}",
            inspection.problems.len(),
            path.display()
        );
        for problem in &inspection.problems {
            println!("  - {}", sanitize_for_terminal(problem));
        }
        return Err(anyhow!("VPK verification failed"));
    }

    println!("OK: valid VPK v1");
    println!("Files: {}", inspection.file_count);
    println!("Mode: {}", inspection.mode.label());
    if inspection.mode == VpkMode::MultiChunk {
        println!("Chunks: {}", inspection.chunk_count);
    }
    println!(
        "Total data: {:.1} MB",
        inspection.total_data_bytes as f64 / 1024.0 / 1024.0
    );

    Ok(())
}

fn run_list(path: &Path) -> Result<()> {
    let inspection = inspect::inspect_vpk(path, false)?;
    for stored_path in &inspection.paths {
        println!("{}", sanitize_for_terminal(stored_path));
    }
    Ok(())
}
