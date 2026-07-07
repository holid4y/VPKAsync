use anyhow::{anyhow, Context, Result};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::config::{load_output_dir, save_output_dir};
use crate::packer::{self, collect::folder_total_size, PackOptions, PackSummary};
use crate::vpk::format::SINGLE_VPK_LIMIT;

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
            output_dir: load_output_dir(),
            mode_choice: ModeChoice::Single,
            running: false,
            logs: vec![
                "Drag and drop an input folder here or click 'Select input folder'.".to_string(),
            ],
            receiver: None,
        }
    }
}

impl VpkGuiApp {
    fn set_input_dir(&mut self, path: PathBuf) {
        let total_size = folder_total_size(&path);
        let exceeds_practical_limit = total_size > SINGLE_VPK_LIMIT;

        self.logs
            .push(format!("Input folder selected: {}", path.display()));
        self.logs.push(format!(
            "Input size: {:.1} MB",
            total_size as f64 / 1024.0 / 1024.0
        ));

        if let Some(warning) = crate::packer::collect::folder_name_warning(&path) {
            self.logs.push(format!("Warning: {warning}"));
        }

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

        let output_name = packer::default_output_base(&input_dir);
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

            let result = packer::package_folder(&input_dir, &out_base, options, &mut logger);
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
                                    summary.skipped.len()
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
            ui.label(
                "Drag and drop an input folder, then choose where to save the generated VPK files.",
            );
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
                        let _ = save_output_dir(&folder);
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

pub fn run() -> Result<()> {
    let viewport = match load_window_icon() {
        Ok(icon) => egui::ViewportBuilder::default()
            .with_inner_size([760.0, 560.0])
            .with_icon(icon),
        Err(err) => {
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
        .max_by_key(|entry| entry.width().saturating_mul(entry.height()))
        .ok_or_else(|| anyhow!("icon.ico does not contain any icon entries"))?;

    let image = best_entry
        .decode()
        .context("Failed to decode icon image from icon.ico")?;

    Ok(egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    })
}
