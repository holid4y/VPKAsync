#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use crc32fast::Hasher;
use eframe::egui;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use walkdir::WalkDir;

const SINGLE_VPK_LIMIT: u64 = 600 * 1024 * 1024;
const CHUNK_SIZE: u64 = 600 * 1024 * 1024;

#[repr(C, packed)]
struct VpkHeader {
    signature: u32,
    version: u32,
    tree_size: u32,
}

struct FileEntry {
    ext: String,
    dir: String,
    stem: String,
    data: Vec<u8>,
    crc32: u32,
    archive_index: u16,
    offset: u32,
    size: u32,
}

#[derive(Parser)]
#[command(
    name = "async_vpk",
    about = "Async/parallel VPK creator for TF2 - replacement for Valve's vpk.exe",
    version = "1.1.0"
)]
struct Cli {
    #[arg(help = "Folder to be packaged into a VPK")]
    input: PathBuf,

    #[arg(short, long, help = "Output base path (without extension). Default: input folder name")]
    output: Option<PathBuf>,

    #[arg(long, conflicts_with = "multi", help = "Force single-file mode even above 600 MB")]
    single: bool,

    #[arg(long, conflicts_with = "single", help = "Force multi-chunk mode even below 600 MB")]
    multi: bool,

    #[arg(
        short,
        long,
        value_parser = clap::value_parser!(NonZeroUsize),
        help = "Number of threads (default: all available CPU cores)"
    )]
    threads: Option<NonZeroUsize>,
}

#[derive(Clone, Copy)]
struct PackOptions {
    single: bool,
    multi: bool,
    threads: Option<usize>,
}

struct PackSummary {
    generated_files: Vec<PathBuf>,
    skipped_files: usize,
    bytes_read: u64,
    use_multi: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModeChoice {
    Single,
    Multi,
}

enum UiMessage {
    Log(String),
    Done(Result<PackSummary>),
}

struct VpkGuiApp {
    input_dir: Option<PathBuf>,
    input_size: Option<u64>,
    output_dir: Option<PathBuf>,
    mode_choice: ModeChoice,
    running: bool,
    logs: Vec<String>,
    receiver: Option<mpsc::Receiver<UiMessage>>,
}

impl Default for VpkGuiApp {
    fn default() -> Self {
        Self {
            input_dir: None,
            input_size: None,
            output_dir: None,
            mode_choice: ModeChoice::Single,
            running: false,
            logs: vec!["Drag and drop an input folder here or click 'Select input folder'.".to_string()],
            receiver: None,
        }
    }
}

impl VpkGuiApp {
    fn set_input_dir(&mut self, path: PathBuf) {
        let total_size = folder_total_size(&path);
        let exceeds_practical_limit = total_size > SINGLE_VPK_LIMIT;

        self.logs.push(format!("Input folder selected: {}", path.display()));
        self.logs.push(format!(
            "Input size: {:.1} MB",
            total_size as f64 / 1024.0 / 1024.0
        ));

        if exceeds_practical_limit {
            self.mode_choice = ModeChoice::Multi;
            self.logs.push(format!(
                "Single-file mode is disabled above {:.0} MB, so multi-chunk was selected automatically.",
                SINGLE_VPK_LIMIT as f64 / 1024.0 / 1024.0
            ));
        }

        self.input_dir = Some(path);
        self.input_size = Some(total_size);
    }

    fn start_pack(&mut self) {
        if self.running {
            return;
        }

        let input_dir = match self.input_dir.clone() {
            Some(p) => p,
            None => {
                self.logs
                    .push("Select an input folder before creating the VPK.".to_string());
                return;
            }
        };

        let output_dir = match self.output_dir.clone() {
            Some(p) => p,
            None => {
                self.logs
                    .push("Select an output folder before creating the VPK.".to_string());
                return;
            }
        };

        if let Some(size) = self.input_size {
            if size > SINGLE_VPK_LIMIT {
                self.mode_choice = ModeChoice::Multi;
            }
        }

        let output_name = default_output_base(&input_dir);
        if output_name.as_os_str().is_empty() {
            self.logs
                .push("Could not derive output file name from the input folder.".to_string());
            return;
        }

        let options = PackOptions {
            single: self.mode_choice == ModeChoice::Single,
            multi: self.mode_choice == ModeChoice::Multi,
            threads: None,
        };

        let out_base = output_dir.join(output_name);
        let (tx, rx) = mpsc::channel::<UiMessage>();

        self.running = true;
        self.receiver = Some(rx);
        self.logs.push("Starting packaging...".to_string());

        std::thread::spawn(move || {
            let logger_tx = tx.clone();
            let mut logger = move |line: String| {
                let _ = logger_tx.send(UiMessage::Log(line));
            };

            let result = package_folder(&input_dir, &out_base, options, &mut logger);
            let _ = tx.send(UiMessage::Done(result));
        });
    }
}

impl eframe::App for VpkGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.receiver {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    UiMessage::Log(line) => self.logs.push(line),
                    UiMessage::Done(result) => {
                        self.running = false;
                        match result {
                            Ok(summary) => {
                                self.logs.push("Completed successfully.".to_string());
                                self.logs.push(format!(
                                    "Mode used: {}",
                                    if summary.use_multi {
                                        "multi-chunk"
                                    } else {
                                        "single-file"
                                    }
                                ));
                                self.logs.push(format!(
                                    "Files skipped due to errors: {}",
                                    summary.skipped_files
                                ));
                                self.logs.push(format!(
                                    "Total read: {:.1} MB",
                                    summary.bytes_read as f64 / 1024.0 / 1024.0
                                ));
                                for file in summary.generated_files {
                                    self.logs.push(format!("Generated: {}", file.display()));
                                }
                            }
                            Err(err) => self.logs.push(format!("ERROR: {}", err)),
                        }
                    }
                }
            }
        }

        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            for file in dropped_files {
                if let Some(path) = file.path {
                    if path.is_dir() {
                        self.set_input_dir(path);
                        break;
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("VPK Maker");
            ui.label("Drag and drop an input folder, then choose where to save the generated VPK files.");
            ui.separator();

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.running, egui::Button::new("Select input folder"))
                    .clicked()
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.set_input_dir(folder);
                    }
                }

                if let Some(input) = &self.input_dir {
                    let name = input
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("(unknown)");
                    ui.label(format!("Selected: {}", name));
                } else {
                    ui.label("Selected: none");
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.running, egui::Button::new("Select output folder"))
                    .clicked()
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.logs
                            .push(format!("Output folder selected: {}", folder.display()));
                        self.output_dir = Some(folder);
                    }
                }

                if let Some(output) = &self.output_dir {
                    let name = output
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("(unknown)");
                    ui.label(format!("Selected: {}", name));
                } else {
                    ui.label("Selected: none");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Package mode:");
                ui.add_enabled_ui(!self.running, |ui| {
                    let single_enabled = self
                        .input_size
                        .map(|size| size <= SINGLE_VPK_LIMIT)
                        .unwrap_or(true);

                    ui.add_enabled_ui(single_enabled, |ui| {
                        ui.radio_value(&mut self.mode_choice, ModeChoice::Single, "Single-file");
                    });
                    ui.radio_value(&mut self.mode_choice, ModeChoice::Multi, "Multi-chunk");
                });
            });

            if let Some(size) = self.input_size {
                if size > SINGLE_VPK_LIMIT {
                    ui.label(format!(
                        "Single-file is unavailable above {:.0} MB, so multi-chunk is enforced.",
                        SINGLE_VPK_LIMIT as f64 / 1024.0 / 1024.0
                    ));
                } else {
                    ui.label("Single-file is available for this input size.");
                }
            }

            if ui
                .add_enabled(
                    !self.running,
                    egui::Button::new("Create VPK").min_size(egui::vec2(120.0, 32.0)),
                )
                .clicked()
            {
                self.start_pack();
            }

            if self.running {
                ui.label("Processing... please wait.");
            }

            ui.separator();
            ui.label("Log:");
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                for line in &self.logs {
                    ui.label(line);
                }
            });
        });
    }
}

fn main() -> Result<()> {
    if std::env::args_os().len() > 1 {
        run_cli()
    } else {
        run_gui()
    }
}

fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let input = cli.input;
    let output = cli.output.unwrap_or_else(|| default_output_base(&input));

    let options = PackOptions {
        single: cli.single,
        multi: cli.multi,
        threads: cli.threads.map(|n| n.get()),
    };

    println!("═══════════════════════════════════════════════");
    println!("  VPK Maker  |  Async/Parallel  |  TF2 compatible  ");
    println!("═══════════════════════════════════════════════");

    let summary = package_folder(&input, &output, options, |line| println!("{}", line))?;

    println!();
    println!("✓ Done!");
    println!(
        "Mode used: {}",
        if summary.use_multi {
            "multi-chunk (_dir + _000.vpk...)"
        } else {
            "single-file (.vpk)"
        }
    );
    println!("Files skipped due to errors: {}", summary.skipped_files);
    for file in summary.generated_files {
        println!("  -> {}", file.display());
    }

    Ok(())
}

fn run_gui() -> Result<()> {
    let viewport = match load_window_icon() {
        Ok(icon) => egui::ViewportBuilder::default()
            .with_inner_size([760.0, 560.0])
            .with_icon(icon),
        Err(err) => {
            // Fallback keeps the app usable even if icon decoding fails.
            eprintln!("Warning: could not load window icon: {err}");
            egui::ViewportBuilder::default().with_inner_size([760.0, 560.0])
        }
    };

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "VPK Tool",
        options,
        Box::new(|_cc| Ok(Box::new(VpkGuiApp::default()))),
    )
    .map_err(|e| anyhow!("Failed to start UI: {}", e))
}

fn load_window_icon() -> Result<egui::IconData> {
    let icon_bytes = include_bytes!("../icon.ico");
    let icon_dir = ico::IconDir::read(std::io::Cursor::new(icon_bytes.as_slice()))
        .context("Failed to parse icon.ico")?;

    let best_entry = icon_dir
        .entries()
        .iter()
        .max_by_key(|entry| {
            let w = u32::from(entry.width());
            let h = u32::from(entry.height());
            w.saturating_mul(h)
        })
        .ok_or_else(|| anyhow!("icon.ico does not contain any icon entries"))?;

    let image = best_entry
        .decode()
        .context("Failed to decode icon image from icon.ico")?;

    Ok(egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: u32::from(image.width()),
        height: u32::from(image.height()),
    })
}

fn default_output_base(input: &Path) -> PathBuf {
    input
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output"))
}

fn folder_total_size(root: &Path) -> u64 {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn package_folder<F>(
    input: &Path,
    out_base: &Path,
    options: PackOptions,
    mut log: F,
) -> Result<PackSummary>
where
    F: FnMut(String),
{
    let input = input
        .canonicalize()
        .with_context(|| format!("Could not open '{}'.", input.display()))?;

    if !input.is_dir() {
        return Err(anyhow!("'{}' is not a valid folder.", input.display()));
    }

    log(format!("Input: {}", input.display()));
    log(format!("Output base: {}", out_base.display()));
    log("[1/3] Collecting files...".to_string());

    let files: Vec<PathBuf> = WalkDir::new(&input)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    if files.is_empty() {
        return Err(anyhow!("No files found in the selected folder."));
    }

    let total_size: u64 = files
        .iter()
        .map(|f| fs::metadata(f).map(|m| m.len()).unwrap_or(0))
        .sum();

    let exceeds_practical_limit = total_size > SINGLE_VPK_LIMIT;

    let requested_multi = if options.single {
        false
    } else if options.multi {
        true
    } else {
        exceeds_practical_limit
    };

    let use_multi = requested_multi || exceeds_practical_limit;

    log(format!("{} files found", files.len()));
    log(format!(
        "Total size: {:.1} MB",
        total_size as f64 / 1024.0 / 1024.0
    ));
    if exceeds_practical_limit && options.single {
        log(format!(
            "Input is above {:.1} MB: forcing multi-chunk mode.",
            SINGLE_VPK_LIMIT as f64 / 1024.0 / 1024.0
        ));
    }
    log(format!(
        "Mode: {}",
        if use_multi {
            "multi-chunk (_dir + _000.vpk...)"
        } else {
            "single-file (.vpk)"
        }
    ));

    log("[2/3] Reading and processing files...".to_string());

    let errors = Mutex::new(Vec::new());
    let bytes_read = AtomicU64::new(0);

    let read_entries = || {
        files
            .par_iter()
            .filter_map(|path| {
                let rel = match path.strip_prefix(&input) {
                    Ok(r) => r,
                    Err(e) => {
                        if let Ok(mut guard) = errors.lock() {
                            guard.push(format!("{}: {}", path.display(), e));
                        }
                        return None;
                    }
                };

                let mut data = Vec::new();
                match File::open(path).and_then(|mut f| f.read_to_end(&mut data)) {
                    Ok(_) => {}
                    Err(e) => {
                        if let Ok(mut guard) = errors.lock() {
                            guard.push(format!("{}: {}", path.display(), e));
                        }
                        return None;
                    }
                }

                let mut hasher = Hasher::new();
                hasher.update(&data);
                let crc32 = hasher.finalize();

                bytes_read.fetch_add(data.len() as u64, Ordering::Relaxed);

                let ext = rel
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let stem = rel
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let dir = rel
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or(" ")
                    .replace('\\', "/");

                let dir = if dir.is_empty() { " ".to_string() } else { dir };
                // VPK v1 stores file size in u32, so larger files are skipped safely.
                if data.len() > u32::MAX as usize {
                    if let Ok(mut guard) = errors.lock() {
                        guard.push(format!(
                            "{}: file is too large for VPK v1 (max {} bytes)",
                            path.display(),
                            u32::MAX
                        ));
                    }
                    return None;
                }

                let size = data.len() as u32;

                Some(FileEntry {
                    ext,
                    dir,
                    stem,
                    data,
                    crc32,
                    archive_index: 0,
                    offset: 0,
                    size,
                })
            })
            .collect::<Vec<FileEntry>>()
    };

    let mut entries = if let Some(threads) = options.threads {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .context("Failed to create custom thread pool")?;
        pool.install(read_entries)
    } else {
        read_entries()
    };

    let errs = errors.into_inner().unwrap_or_default();
    if !errs.is_empty() {
        log(format!("{} file(s) were skipped due to errors.", errs.len()));
    }

    let total_read = bytes_read.load(Ordering::Relaxed);
    log(format!(
        "Total read: {:.1} MB",
        total_read as f64 / 1024.0 / 1024.0
    ));

    log("[3/3] Writing VPK...".to_string());
    let generated_files = if use_multi {
        write_multi_vpk(&mut entries, out_base)?
    } else {
        vec![write_single_vpk(&mut entries, out_base)?]
    };

    Ok(PackSummary {
        generated_files,
        skipped_files: errs.len(),
        bytes_read: total_read,
        use_multi,
    })
}

fn write_single_vpk(entries: &mut Vec<FileEntry>, out_base: &Path) -> Result<PathBuf> {
    for e in entries.iter_mut() {
        e.archive_index = 0x7FFF;
        e.offset = 0;
    }

    let out_path = out_base.with_extension("vpk");
    let file = File::create(&out_path)?;
    let mut writer = BufWriter::new(file);

    let (tree_bytes, data_bytes) = build_tree_and_embedded(entries);

    let header = VpkHeader {
        signature: 0x55AA1234,
        version: 1,
        tree_size: tree_bytes.len() as u32,
    };

    writer.write_all(&header.signature.to_le_bytes())?;
    writer.write_all(&header.version.to_le_bytes())?;
    writer.write_all(&header.tree_size.to_le_bytes())?;
    writer.write_all(&tree_bytes)?;
    writer.write_all(&data_bytes)?;
    writer.flush()?;

    Ok(out_path)
}

fn write_multi_vpk(entries: &mut Vec<FileEntry>, out_base: &Path) -> Result<Vec<PathBuf>> {
    let mut chunks: Vec<Vec<u8>> = vec![Vec::new()];
    let mut output_files = Vec::new();

    for e in entries.iter_mut() {
        let current_chunk = chunks.len() - 1;
        let current_size = chunks[current_chunk].len() as u64;

        if current_size + e.size as u64 > CHUNK_SIZE && current_size > 0 {
            chunks.push(Vec::new());
        }

        let chunk_idx = chunks.len() - 1;
        e.archive_index = chunk_idx as u16;
        e.offset = chunks[chunk_idx].len() as u32;
        chunks[chunk_idx].extend_from_slice(&e.data);
    }

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_path = PathBuf::from(format!("{}_{:03}.vpk", out_base.display(), i));
        fs::write(&chunk_path, chunk)?;
        output_files.push(chunk_path);
    }

    let dir_path = PathBuf::from(format!("{}_dir.vpk", out_base.display()));
    let file = File::create(&dir_path)?;
    let mut writer = BufWriter::new(file);

    let (tree_bytes, _) = build_tree_and_embedded(entries);

    let header = VpkHeader {
        signature: 0x55AA1234,
        version: 1,
        tree_size: tree_bytes.len() as u32,
    };

    writer.write_all(&header.signature.to_le_bytes())?;
    writer.write_all(&header.version.to_le_bytes())?;
    writer.write_all(&header.tree_size.to_le_bytes())?;
    writer.write_all(&tree_bytes)?;
    writer.flush()?;

    output_files.push(dir_path);
    Ok(output_files)
}

fn build_tree_and_embedded(entries: &[FileEntry]) -> (Vec<u8>, Vec<u8>) {
    let mut tree: HashMap<&str, HashMap<&str, Vec<&FileEntry>>> = HashMap::new();

    for e in entries {
        tree.entry(&e.ext)
            .or_default()
            .entry(&e.dir)
            .or_default()
            .push(e);
    }

    let mut dir_buf: Vec<u8> = Vec::new();
    let mut data_buf: Vec<u8> = Vec::new();

    // Sort keys to keep output stable across runs.
    let mut exts: Vec<&str> = tree.keys().copied().collect();
    exts.sort_unstable();

    for ext in exts {
        let dirs = match tree.get(ext) {
            Some(d) => d,
            None => continue,
        };

        write_cstring(&mut dir_buf, ext);

        let mut dir_keys: Vec<&str> = dirs.keys().copied().collect();
        dir_keys.sort_unstable();

        for dir in dir_keys {
            let files = match dirs.get(dir) {
                Some(f) => f,
                None => continue,
            };

            write_cstring(&mut dir_buf, dir);

            let mut sorted_files = files.clone();
            sorted_files.sort_by(|a, b| a.stem.cmp(&b.stem));

            for f in sorted_files {
                write_cstring(&mut dir_buf, &f.stem);

                dir_buf.extend_from_slice(&f.crc32.to_le_bytes());
                dir_buf.extend_from_slice(&0u16.to_le_bytes());
                dir_buf.extend_from_slice(&f.archive_index.to_le_bytes());

                if f.archive_index == 0x7FFF {
                    let offset = data_buf.len() as u32;
                    dir_buf.extend_from_slice(&offset.to_le_bytes());
                    dir_buf.extend_from_slice(&f.size.to_le_bytes());
                    data_buf.extend_from_slice(&f.data);
                } else {
                    dir_buf.extend_from_slice(&f.offset.to_le_bytes());
                    dir_buf.extend_from_slice(&f.size.to_le_bytes());
                }

                dir_buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
            }

            dir_buf.push(0);
        }

        dir_buf.push(0);
    }

    dir_buf.push(0);

    (dir_buf, data_buf)
}

fn write_cstring(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}
