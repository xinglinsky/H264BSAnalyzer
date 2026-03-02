//! egui-based GUI: layout aligned with original (left: NAL table + Hex; right: File info + Tree).
//! Supports dark and light theme with theme-aware panel colors.
//! Recent file list (persisted in config dir) like flv_parser.

mod config;

use eframe::egui;
use std::path::{Path, PathBuf};

use crate::decode::{DecodedFrame, Decoder};
use crate::export::{export_bmp, export_jpeg, export_rgb, export_yuv};
use crate::model::{FileType, NaluInfo, NalUnitType, ParseResult, SliceType};
use crate::parser::parse_file;
use crate::parser::h265_nal_type_name;
use crate::tree_text_for_nal;
use crate::tree_text_for_nal_h265;

/// UI theme: dark (default) or light.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    fn is_dark(self) -> bool {
        matches!(self, Theme::Dark)
    }
}

pub struct App {
    path: Option<PathBuf>,
    result: Option<ParseResult>,
    selected_nal_index: Option<usize>,
    error: Option<String>,
    decoder: Option<Decoder>,
    current_frame: Option<DecodedFrame>,
    playback_playing: bool,
    theme: Theme,
    /// Recent file paths, max 10, newest first; persisted to config.
    recent_file_paths: Vec<PathBuf>,
    help_show_about: bool,
    help_show_license: bool,
    help_show_shortcuts: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            path: None,
            result: None,
            selected_nal_index: None,
            error: None,
            decoder: None,
            current_frame: None,
            playback_playing: false,
            theme: Theme::Dark,
            recent_file_paths: Vec::new(),
            help_show_about: false,
            help_show_license: false,
            help_show_shortcuts: false,
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::default();
        if let Some(storage) = cc.storage.as_ref() {
            if let Some(s) = storage.get_string("theme") {
                app.theme = if s == "light" { Theme::Light } else { Theme::Dark };
            }
        }
        let cfg = config::load_config();
        for s in cfg.recent_paths.iter().take(10) {
            app.recent_file_paths.push(PathBuf::from(s));
        }
        app
    }

    fn push_recent_path(&mut self, path: PathBuf) {
        self.recent_file_paths.retain(|p| p != &path);
        self.recent_file_paths.insert(0, path);
        if self.recent_file_paths.len() > 10 {
            self.recent_file_paths.truncate(10);
        }
        self.save_recent_paths();
    }

    fn save_recent_paths(&self) {
        let mut cfg = config::load_config();
        cfg.recent_paths = self
            .recent_file_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        config::save_config(&cfg);
    }

    fn load_file(&mut self, path: PathBuf) {
        self.error = None;
        self.path = Some(path.clone());
        self.selected_nal_index = None;
        self.decoder = None;
        self.current_frame = None;
        self.playback_playing = false;
        match parse_file(&path) {
            Ok(r) => {
                self.result = Some(r);
                self.push_recent_path(path);
            }
            Err(e) => {
                self.result = None;
                self.error = Some(e.to_string());
                self.recent_file_paths.retain(|p| p != &path);
                self.save_recent_paths();
            }
        }
    }

    fn start_playback(&mut self) {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => return,
        };
        self.decoder = None;
        self.current_frame = None;
        self.playback_playing = false;
        match Decoder::open(&path) {
            Ok(mut dec) => {
                let frame = dec.next_frame().ok().and_then(|f| f);
                self.current_frame = frame;
                self.decoder = Some(dec);
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    fn stop_playback(&mut self) {
        self.decoder = None;
        self.current_frame = None;
        self.playback_playing = false;
    }

    fn next_frame(&mut self) {
        if let Some(ref mut dec) = self.decoder {
            match dec.next_frame() {
                Ok(Some(f)) => self.current_frame = Some(f),
                Ok(None) => {
                    if let Ok(Some(f)) = dec.flush_frames() {
                        self.current_frame = Some(f);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Truncate path for title bar: keep start and filename, replace middle with "...".
fn truncate_path_for_title(path: &Path, max_len: usize) -> String {
    let path_str = path.display().to_string();
    if path_str.chars().count() <= max_len {
        return path_str;
    }
    let filename = path
        .file_name()
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();
    let filename_len = filename.chars().count();
    const ELLIPSIS: &str = "…";
    let ellipsis_len = ELLIPSIS.chars().count();
    if filename_len + ellipsis_len >= max_len {
        let take = max_len.saturating_sub(ellipsis_len + 1);
        let truncated: String = filename.chars().take(take).collect();
        return format!("{}{}", truncated, ELLIPSIS);
    }
    let prefix_len = max_len - ellipsis_len - filename_len;
    let prefix: String = path_str.chars().take(prefix_len).collect();
    format!("{}…{}", prefix, filename)
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", if self.theme == Theme::Light { "light".to_string() } else { "dark".to_string() });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let visuals = if self.theme.is_dark() {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        ctx.set_visuals(visuals);

        let system_theme = if self.theme.is_dark() {
            egui::viewport::SystemTheme::Dark
        } else {
            egui::viewport::SystemTheme::Light
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(system_theme));

        let title = match &self.path {
            Some(p) => format!("H264BSAnalyzer - {}", truncate_path_for_title(p.as_path(), 72)),
            None => "H264BSAnalyzer".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
                    if let Some(p) = f.path {
                        if p.extension().map_or(false, |e| {
                            let s = e.to_string_lossy();
                            s.eq_ignore_ascii_case("h264")
                                || s.eq_ignore_ascii_case("264")
                                || s.eq_ignore_ascii_case("avc")
                                || s.eq_ignore_ascii_case("h265")
                                || s.eq_ignore_ascii_case("265")
                                || s.eq_ignore_ascii_case("hevc")
                        }) {
                            self.load_file(p);
                            break;
                        }
                    }
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…").clicked() {
                        ui.close_menu();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("H.264/H.265", &["h264", "264", "avc", "h265", "265", "hevc"])
                            .pick_file()
                        {
                            self.load_file(p);
                        }
                    }
                    if !self.recent_file_paths.is_empty() {
                        ui.separator();
                        ui.menu_button("Recent", |ui| {
                            ui.set_min_width(200.0);
                            let recent: Vec<PathBuf> = self.recent_file_paths.iter().take(10).cloned().collect();
                            for path in recent {
                                let label = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                let full = path.display().to_string();
                                if ui.button(&label).on_hover_text(&full).clicked() {
                                    ui.close_menu();
                                    self.load_file(path);
                                }
                            }
                        });
                    }
                    ui.separator();
                    if ui.button("Close").clicked() {
                        ui.close_menu();
                        self.path = None;
                        self.result = None;
                        self.selected_nal_index = None;
                        self.error = None;
                    }
                });
                if ui.button("Play").clicked() {
                    self.start_playback();
                }
                ui.menu_button("Export", |ui| {
                    if let Some(ref frame) = self.current_frame {
                        if ui.button("Save current frame as BMP…").clicked() {
                            ui.close_menu();
                            if let Some(p) = rfd::FileDialog::new().add_filter("BMP", &["bmp"]).save_file() {
                                if let Err(e) = export_bmp(&p, frame) {
                                    self.error = Some(e);
                                }
                            }
                        }
                        if ui.button("Save current frame as JPEG…").clicked() {
                            ui.close_menu();
                            if let Some(p) = rfd::FileDialog::new().add_filter("JPEG", &["jpg", "jpeg"]).save_file() {
                                if let Err(e) = export_jpeg(&p, frame) {
                                    self.error = Some(e);
                                }
                            }
                        }
                        if ui.button("Save current frame as YUV…").clicked() {
                            ui.close_menu();
                            if let Some(p) = rfd::FileDialog::new().add_filter("YUV", &["yuv"]).save_file() {
                                if let Err(e) = export_yuv(&p, frame) {
                                    self.error = Some(e);
                                }
                            }
                        }
                        if ui.button("Save current frame as RGB…").clicked() {
                            ui.close_menu();
                            if let Some(p) = rfd::FileDialog::new().add_filter("RGB", &["rgb"]).save_file() {
                                if let Err(e) = export_rgb(&p, frame) {
                                    self.error = Some(e);
                                }
                            }
                        }
                    } else {
                        ui.add_enabled(false, egui::Button::new("Save current frame as… (decode a frame first)"));
                    }
                });
                ui.menu_button("View", |ui| {
                    let theme_label = format!("Theme ({})", if self.theme == Theme::Dark { "Dark" } else { "Light" });
                    ui.menu_button(theme_label, |ui| {
                        if ui.selectable_label(self.theme == Theme::Dark, "Dark").clicked() {
                            self.theme = Theme::Dark;
                            ui.close_menu();
                        }
                        if ui.selectable_label(self.theme == Theme::Light, "Light").clicked() {
                            self.theme = Theme::Light;
                            ui.close_menu();
                        }
                    });
                });
                ui.menu_button("Help", |ui| {
                    ui.set_min_width(180.0);
                    if ui.button("About").on_hover_text("Application info and version").clicked() {
                        ui.close_menu();
                        self.help_show_about = true;
                    }
                    if ui.button("License").on_hover_text("MIT License text").clicked() {
                        ui.close_menu();
                        self.help_show_license = true;
                    }
                    if ui.button("Keyboard shortcuts").on_hover_text("Shortcut reference").clicked() {
                        ui.close_menu();
                        self.help_show_shortcuts = true;
                    }
                });
            });
        });

        if let Some(ref err) = self.error {
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.colored_label(egui::Color32::RED, err);
            });
        }

        if self.help_show_about {
            egui::Window::new("About")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("H264BSAnalyzer");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(4.0);
                    ui.label("H.264 / H.265 bitstream analyzer — NAL list, hex view, SPS/PPS/VPS/slice parsing.");
                    ui.add_space(4.0);
                    ui.label("MIT License");
                    ui.add_space(8.0);
                    if ui.button("OK").clicked() {
                        self.help_show_about = false;
                    }
                });
        }
        if self.help_show_license {
            egui::Window::new("License")
                .default_width(420.0)
                .default_height(320.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(include_str!("../../LICENSE"));
                    });
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() {
                        self.help_show_license = false;
                    }
                });
        }
        if self.help_show_shortcuts {
            egui::Window::new("Keyboard shortcuts")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("File → Open: open H.264/H.265 file");
                    ui.label("File → Recent: reopen a recent file");
                    ui.label("NAL list: click a row to select and view hex / parsing detail");
                    ui.label("View → Theme: switch Dark / Light");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.help_show_shortcuts = false;
                    }
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.path.is_some() {
                ui.horizontal(|ui| {
                    if self.decoder.is_some() {
                        if ui.button("Stop").clicked() {
                            self.stop_playback();
                        }
                        if ui.button("Next frame").clicked() {
                            self.next_frame();
                        }
                    }
                });
            }

            if let Some(ref frame) = self.current_frame {
                let (w, h) = (frame.width as usize, frame.height as usize);
                if w > 0 && h > 0 && frame.rgb.len() >= w * h * 3 {
                    let color_image = egui::ColorImage {
                        size: [w, h],
                        pixels: frame
                            .rgb
                            .chunks_exact(3)
                            .map(|c| egui::Color32::from_rgb(c[0], c[1], c[2]))
                            .collect(),
                    };
                    let texture = ctx.load_texture(
                        "playback_frame",
                        color_image,
                        egui::TextureOptions::default(),
                    );
                    ui.image(egui::ImageSource::Texture(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::Vec2::new(w as f32, h as f32),
                    )));
                }
            }

            if let Some(ref res) = self.result {
                let avail = ui.available_rect_before_wrap();
                let nal_table_min_w = NAL_COL_NO + NAL_COL_OFFSET + NAL_COL_LEN + NAL_COL_START + NAL_COL_TYPE + NAL_COL_INFO + NAL_CELL_PAD * 2.0;
                let left_w = (avail.width() * 0.60).max(280.0).max(nal_table_min_w);
                let col_type_w = NAL_COL_TYPE + (left_w - nal_table_min_w).max(0.0);
                let right_w = (avail.width() - left_w - 8.0).max(260.0);
                let full_h = avail.height();
                let nal_table_h = (full_h * 0.55).max(200.0).min(full_h - 140.0);
                let hex_view_h = (full_h - nal_table_h - 32.0).max(100.0);

                ui.horizontal(|ui| {
                    ui.set_min_width(avail.width());

                    ui.vertical(|ui| {
                        ui.set_min_height(full_h);
                        ui.set_min_width(left_w);
                        ui.set_max_width(left_w);

                        ui.add_space(2.0);
                        {
                            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                            let (rect, _) = ui.allocate_exact_size(
                                egui::Vec2::new(left_w, row_h),
                                egui::Sense::hover(),
                            );
                            let font_id = egui::FontId::proportional(12.0);
                            let fg = ui.visuals().text_color();
                            let mut x = rect.min.x + NAL_CELL_PAD;
                            let y = rect.min.y + 2.0;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "No.", font_id.clone(), fg);
                            x += NAL_COL_NO;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "Offset", font_id.clone(), fg);
                            x += NAL_COL_OFFSET;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "Length", font_id.clone(), fg);
                            x += NAL_COL_LEN;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "Start Code", font_id.clone(), fg);
                            x += NAL_COL_START;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "NAL Type", font_id.clone(), fg);
                            x += col_type_w;
                            ui.painter().text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, "Info", font_id.clone(), fg);
                        }
                        if let Some(clicked) = paint_nal_table(ui, res, self.selected_nal_index, nal_table_h, self.theme.is_dark(), left_w) {
                            self.selected_nal_index = Some(clicked);
                        }
                        ui.add_space(4.0);
                        ui.label("Hex View");
                        let hex_w = left_w;
                        let (hex_rect, _) = ui.allocate_exact_size(
                            egui::Vec2::new(hex_w, hex_view_h),
                            egui::Sense::hover(),
                        );
                        ui.allocate_new_ui(
                            egui::UiBuilder::default().max_rect(hex_rect),
                            |ui| {
                            ui.set_min_width(hex_w);
                            if let Some(i) = self.selected_nal_index {
                                if i < res.nalus.len() {
                                    let hex_str = hex_dump(&res.nalus[i].raw, 16);
                                    egui::ScrollArea::vertical()
                                        .max_height(hex_view_h - 4.0)
                                        .show(ui, |ui| {
                                            ui.set_min_width(hex_w - 8.0);
                                            ui.monospace(hex_str);
                                        });
                                } else {
                                    ui.weak("Select a NAL");
                                }
                            } else {
                                ui.weak("Select a NAL");
                            }
                            },
                        );
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        ui.set_min_width(right_w);
                        ui.set_max_width(right_w);
                        ui.add_space(2.0);
                        ui.strong("File Information");
                        paint_file_info(ui, res);
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        let remaining_h = ui.available_rect_before_wrap().height();
                        egui::ScrollArea::vertical()
                            .max_height(remaining_h)
                            .show(ui, |ui| {
                                ui.set_max_width(right_w);
                                egui::CollapsingHeader::new("NAL Parsing Information")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        if let Some(idx) = self.selected_nal_index {
                                            if idx < res.nalus.len() {
                                                let detail = match res.file_type {
                                                    FileType::H265 => tree_text_for_nal_h265(&res.nalus[idx].raw),
                                                    _ => tree_text_for_nal(&res.nalus[idx].raw),
                                                };
                                                paint_nal_detail_foldable(ui, &detail, right_w);
                                            } else {
                                                ui.weak("Select a NAL");
                                            }
                                        } else {
                                            ui.weak("Select a NAL");
                                        }
                                    });
                                ui.add_space(24.0);
                            });
                    });
                });
            } else {
                paint_start_page(ui, self);
            }
        });
    }
}

/// Starting page when no file is loaded: welcome card with open button and short guidance.
fn paint_start_page(ui: &mut egui::Ui, app: &mut App) {
    let avail = ui.available_rect_before_wrap();
    let card_max_w = 420.0_f32.min(avail.width() - 48.0);
    let card_min_w = 320.0_f32;
    let (rect, _) = ui.allocate_exact_size(avail.size(), egui::Sense::hover());
    let center = rect.center();
    let card_rect = egui::Rect::from_center_size(
        center,
        egui::Vec2::new(card_max_w.max(card_min_w), (avail.height() * 0.5).max(280.0).min(avail.height() - 24.0)),
    );
    let rounding = 12.0;
    let stroke = ui.visuals().widgets.noninteractive.fg_stroke;
    let fill = ui.visuals().faint_bg_color;
    ui.painter().rect_filled(card_rect, rounding, fill);
    ui.painter().rect_stroke(card_rect, rounding, stroke);

    let inner = card_rect.shrink(24.0);
    ui.allocate_new_ui(egui::UiBuilder::default().max_rect(inner), |ui| {
        ui.set_max_width(inner.width());
        ui.add_space(20.0);
        ui.heading(egui::RichText::new("H264BSAnalyzer").size(22.0));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("H.264 / H.265 bitstream analyzer").color(ui.visuals().weak_text_color()));
        ui.add_space(24.0);
        let open_clicked = ui.add(egui::Button::new(egui::RichText::new("Open file…").size(14.0)).min_size(egui::Vec2::new(160.0, 32.0))).clicked();
        if open_clicked {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("H.264/H.265", &["h264", "264", "avc", "h265", "265", "hevc"])
                .pick_file()
            {
                app.load_file(p);
            }
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("or drag a file into this window").color(ui.visuals().weak_text_color()));
        if !app.recent_file_paths.is_empty() {
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Recent").size(12.0).color(ui.visuals().weak_text_color()));
            ui.add_space(4.0);
            let recent: Vec<PathBuf> = app.recent_file_paths.iter().take(10).cloned().collect();
            let full_w = ui.available_width();
            const MAX_NAME_CHARS: usize = 22;
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    ui.set_min_width(full_w);
                    for chunk in recent.chunks(2) {
                        ui.horizontal(|ui| {
                            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                            for path in chunk {
                                let label = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                let display = if label.chars().count() > MAX_NAME_CHARS {
                                    format!("{}…", label.chars().take(MAX_NAME_CHARS - 1).collect::<String>())
                                } else {
                                    label.clone()
                                };
                                let full = path.display().to_string();
                                ui.scope(|ui| {
                                    ui.set_min_width(half);
                                    ui.set_max_width(half);
                                    if ui.link(&display).on_hover_text(&full).clicked() {
                                        app.load_file(path.clone());
                                    }
                                });
                            }
                        });
                    }
                });
            ui.add_space(8.0);
        }
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Supported: .h264 .264 .avc .h265 .265 .hevc").size(12.0).color(ui.visuals().weak_text_color()));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.label("• NAL list, hex view");
            ui.label("• File / NAL info");
            ui.label("• Theme, recent files");
        });
    });
}

fn nal_type_description(n: &NaluInfo) -> &'static str {
    if let Some(t) = n.h265_nal_type {
        return h265_nal_type_name(t);
    }
    match n.nal_type {
        NalUnitType::Sps => "Sequence parameter set",
        NalUnitType::Pps => "Picture parameter set",
        NalUnitType::Sei => "Supplemental enhancement info...",
        NalUnitType::IdrSlice => "Coded slice of an IDR picture",
        NalUnitType::NonIdrSlice => "Coded slice of a non-IDR picture",
        NalUnitType::Filler => "FILLER_DATA",
        NalUnitType::Aud => "Access unit delimiter",
        _ => "NAL unit",
    }
}

fn nal_info_short(res: &ParseResult, index: usize) -> String {
    let n = &res.nalus[index];
    if let Some(t) = n.h265_nal_type {
        return match t {
            32 => "VPS".to_string(),
            33 => "SPS".to_string(),
            34 => "PPS".to_string(),
            39 => "SEI".to_string(),
            6 | 7 | 8 | 9 | 16 | 17 | 18 | 19 | 20 => format!("IDR #{}", idr_count_h265(res, index)),
            1 | 3 | 5 => format!("P Slice #{}", p_slice_count_h265(res, index)),
            0 | 2 | 4 => format!("B Slice #{}", b_slice_count_h265(res, index)),
            _ => format!("Type {}", t),
        };
    }
    match n.nal_type {
        NalUnitType::Sps => "SPS".to_string(),
        NalUnitType::Pps => "PPS".to_string(),
        NalUnitType::Sei => "SEI".to_string(),
        NalUnitType::IdrSlice => format!("IDR #{}", idr_count(res, index)),
        NalUnitType::NonIdrSlice => match n.slice_type {
            SliceType::P => format!("P Slice #{}", p_slice_count(res, index)),
            SliceType::B => format!("B Slice #{}", b_slice_count(res, index)),
            SliceType::I => format!("I Slice #{}", i_slice_count(res, index)),
            _ => format!("Slice #{}", slice_count(res, index)),
        },
        NalUnitType::Filler => "FILLER".to_string(),
        NalUnitType::Aud => "AUD".to_string(),
        _ => "".to_string(),
    }
}

fn idr_count(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| n.nal_type == NalUnitType::IdrSlice || n.h265_nal_type == Some(0) || n.h265_nal_type == Some(18))
        .count()
        .saturating_sub(1)
}

fn idr_count_h265(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    const H265_IRAP: &[u8] = &[6, 7, 8, 9, 16, 17, 18, 19, 20];
    res.nalus[..=end]
        .iter()
        .filter(|n| n.h265_nal_type.map(|t| H265_IRAP.contains(&t)).unwrap_or(false))
        .count()
        .saturating_sub(1)
}

fn p_slice_count(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| (n.nal_type == NalUnitType::NonIdrSlice && n.slice_type == SliceType::P) || n.h265_nal_type == Some(1) || n.h265_nal_type == Some(19))
        .count()
}

fn p_slice_count_h265(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| n.h265_nal_type == Some(1) || n.h265_nal_type == Some(3) || n.h265_nal_type == Some(5))
        .count()
}

fn b_slice_count_h265(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| n.h265_nal_type == Some(0) || n.h265_nal_type == Some(2) || n.h265_nal_type == Some(4))
        .count()
}

fn b_slice_count(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| n.nal_type == NalUnitType::NonIdrSlice && n.slice_type == SliceType::B)
        .count()
}

fn i_slice_count(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| n.nal_type == NalUnitType::NonIdrSlice && n.slice_type == SliceType::I)
        .count()
}

fn slice_count(res: &ParseResult, up_to: usize) -> usize {
    let end = up_to.min(res.nalus.len().saturating_sub(1));
    res.nalus[..=end]
        .iter()
        .filter(|n| matches!(n.nal_type, NalUnitType::NonIdrSlice | NalUnitType::IdrSlice))
        .count()
}

const NAL_COL_NO: f32 = 32.0;
const NAL_COL_OFFSET: f32 = 72.0;
const NAL_COL_LEN: f32 = 56.0;
const NAL_COL_START: f32 = 72.0;
const NAL_COL_TYPE: f32 = 288.0;
const NAL_COL_INFO: f32 = 92.0;
const NAL_CELL_PAD: f32 = 4.0;

/// Truncate string to at most max_chars, append … if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars.saturating_sub(1)).collect::<String>())
    }
}

/// Theme-aware NAL row colors: (bg, fg) for dark or light theme.
fn nal_row_colors(n: &NaluInfo, is_selected: bool, dark: bool) -> (egui::Color32, egui::Color32) {
    use egui::Color32;
    let (white, black) = (Color32::WHITE, Color32::BLACK);
    if is_selected {
        return if dark {
            (Color32::from_rgb(0x8b, 0x2e, 0x2e), white)
        } else {
            (Color32::from_rgb(0xc0, 0x50, 0x50), white)
        };
    }
    if dark {
        if let Some(t) = n.h265_nal_type {
            return match t {
                32 => (Color32::from_rgb(0x55, 0x44, 0x22), Color32::from_rgb(0xee, 0xdd, 0xaa)),
                33 => (Color32::from_rgb(0x55, 0x44, 0x33), Color32::from_rgb(0xff, 0xdd, 0xbb)),
                34 => (Color32::from_rgb(0x44, 0x44, 0x33), Color32::from_rgb(0xdd, 0xee, 0xcc)),
                39 => (Color32::from_rgb(0x2a, 0x3a, 0x4a), Color32::from_rgb(0xaa, 0xee, 0xff)),
                0 | 18 => (Color32::from_rgb(0x2a, 0x4a, 0x2a), Color32::from_rgb(0xaa, 0xff, 0xaa)),
                1 | 19 => (Color32::from_rgb(0x2a, 0x35, 0x4a), Color32::from_rgb(0xbb, 0xdd, 0xff)),
                _ => (Color32::from_rgb(0x35, 0x32, 0x42), Color32::from_rgb(0xdd, 0xd0, 0xee)),
            };
        }
        match n.nal_type {
            NalUnitType::Sps => (Color32::from_rgb(0x55, 0x44, 0x33), Color32::from_rgb(0xff, 0xdd, 0xbb)),
            NalUnitType::Pps => (Color32::from_rgb(0x44, 0x44, 0x33), Color32::from_rgb(0xdd, 0xee, 0xcc)),
            NalUnitType::Sei => (Color32::from_rgb(0x2a, 0x3a, 0x4a), Color32::from_rgb(0xaa, 0xdd, 0xff)),
            NalUnitType::IdrSlice => (Color32::from_rgb(0x2a, 0x4a, 0x2a), Color32::from_rgb(0xaa, 0xff, 0xaa)),
            NalUnitType::NonIdrSlice => (Color32::from_rgb(0x2a, 0x35, 0x4a), Color32::from_rgb(0xbb, 0xdd, 0xff)),
            NalUnitType::Aud => (Color32::from_rgb(0x2a, 0x4a, 0x4a), Color32::from_rgb(0xaa, 0xff, 0xee)),
            NalUnitType::Filler => (Color32::from_rgb(0x40, 0x40, 0x40), Color32::from_rgb(0xcc, 0xcc, 0xcc)),
            _ => (Color32::from_rgb(0x35, 0x32, 0x42), Color32::from_rgb(0xdd, 0xd0, 0xee)),
        }
    } else {
        if let Some(t) = n.h265_nal_type {
            return match t {
                32 => (Color32::from_rgb(0xff, 0xcc, 0x66), black),
                33 => (Color32::from_rgb(0xff, 0xcc, 0x99), black),
                34 => (Color32::from_rgb(0xff, 0xdd, 0xaa), black),
                39 => (Color32::from_rgb(0xaa, 0xdd, 0xff), black),
                0 | 18 => (Color32::from_rgb(0x99, 0xee, 0x99), black),
                1 | 19 => (Color32::from_rgb(0xcc, 0xee, 0xff), black),
                _ => (Color32::from_rgb(0xe8, 0xe0, 0xf0), black),
            };
        }
        match n.nal_type {
            NalUnitType::Sps => (Color32::from_rgb(0xff, 0xcc, 0x99), black),
            NalUnitType::Pps => (Color32::from_rgb(0xff, 0xdd, 0xaa), black),
            NalUnitType::Sei => (Color32::from_rgb(0xaa, 0xdd, 0xff), black),
            NalUnitType::IdrSlice => (Color32::from_rgb(0x99, 0xee, 0x99), black),
            NalUnitType::NonIdrSlice => (Color32::from_rgb(0xcc, 0xee, 0xff), black),
            NalUnitType::Aud => (Color32::from_rgb(0xaa, 0xff, 0xee), black),
            NalUnitType::Filler => (Color32::from_rgb(0xdd, 0xdd, 0xdd), black),
            _ => (Color32::from_rgb(0xe8, 0xe0, 0xf0), black),
        }
    }
}

fn paint_nal_table(
    ui: &mut egui::Ui,
    res: &ParseResult,
    selected: Option<usize>,
    max_height: f32,
    dark: bool,
    table_available_width: f32,
) -> Option<usize> {
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut clicked = None;
    let scroll_id = ui.id().with("nal_list_scroll");
    const TYPE_CHARS: usize = 34;
    const INFO_CHARS: usize = 14;
    let base_width = NAL_COL_NO + NAL_COL_OFFSET + NAL_COL_LEN + NAL_COL_START + NAL_COL_TYPE + NAL_COL_INFO + NAL_CELL_PAD * 2.0;
    let full_width = table_available_width.max(base_width);
    let col_type_w = NAL_COL_TYPE + (full_width - base_width).max(0.0);
    egui::ScrollArea::vertical()
        .id_salt(scroll_id)
        .max_height(max_height)
        .min_scrolled_height(max_height)
        .drag_to_scroll(true)
        .show_rows(ui, row_h, res.nalus.len(), |ui, row_range| {
            let font_id = egui::FontId::monospace(12.0);
            for i in row_range {
                ui.push_id(i, |ui| {
                    let n = &res.nalus[i];
                    let is_selected = selected == Some(i);
                    let (bg, fg) = nal_row_colors(n, is_selected, dark);
                    let start_code_hex = if n.raw.len() >= n.start_code_len as usize {
                        n.raw[..n.start_code_len as usize]
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<String>()
                    } else {
                        String::new()
                    };
                    let col_type = truncate_str(nal_type_description(n), TYPE_CHARS);
                    let col_info = truncate_str(&nal_info_short(res, i), INFO_CHARS);
                    let len_str = n.raw.len().to_string();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::Vec2::new(full_width, row_h),
                        egui::Sense::click(),
                    );
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    ui.painter().rect_filled(rect, 0.0, bg);
                    let y = rect.min.y + 2.0;
                    let mut x = rect.min.x + NAL_CELL_PAD;
                    let clip = rect;
                    let painter = ui.painter().with_clip_rect(clip);

                    let mut draw_cell = |cell_w: f32, text: &str| {
                        let cell = egui::Rect::from_min_size(egui::Pos2::new(x, rect.min.y), egui::Vec2::new(cell_w, row_h));
                        let p = painter.with_clip_rect(cell);
                        p.text(egui::Pos2::new(x, y), egui::Align2::LEFT_TOP, text, font_id.clone(), fg);
                        x += cell_w;
                    };

                    draw_cell(NAL_COL_NO, &format!("{}", i));
                    draw_cell(NAL_COL_OFFSET, &format!("{:08x}", n.offset));
                    draw_cell(NAL_COL_LEN, &len_str);
                    draw_cell(NAL_COL_START, &start_code_hex);
                    draw_cell(col_type_w, &col_type);
                    draw_cell(NAL_COL_INFO, &col_info);
                });
            }
        });
    clicked
}

/// Parse NAL detail text into sections (title, lines) for foldable UI.
fn nal_detail_sections(detail: &str) -> Vec<(String, Vec<&str>)> {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in detail.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let is_subheader = line.starts_with("  ")
            && {
                let rest = line.trim_start_matches(' ');
                rest.ends_with(':') && !rest[..rest.len().saturating_sub(1)].contains(' ')
            };
        if is_subheader {
            if let Some((name, lines)) = current.take() {
                if !lines.is_empty() {
                    sections.push((name, lines));
                }
            }
            let rest = line.trim_start_matches(' ');
            let name = rest.trim_end_matches(':').to_string();
            current = Some((name, Vec::new()));
            continue;
        }
        match &mut current {
            Some((_, lines)) => lines.push(line),
            None => {
                let name = if line.starts_with("  ") {
                    "NAL Unit".to_string()
                } else {
                    line.trim_end_matches(':').to_string()
                };
                current = Some((name, vec![line]));
            }
        }
    }
    if let Some((name, lines)) = current {
        if !lines.is_empty() {
            sections.push((name, lines));
        }
    }
    if sections.is_empty() && !detail.lines().next().unwrap_or("").is_empty() {
        sections.push(("Detail".to_string(), detail.lines().collect()));
    }
    sections
}

fn paint_nal_detail_foldable(ui: &mut egui::Ui, detail: &str, _max_w: f32) {
    let sections = nal_detail_sections(detail);
    if sections.is_empty() {
        ui.monospace(detail);
        ui.add_space(16.0);
        return;
    }
    for (title, lines) in sections {
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| {
                for line in lines {
                    ui.monospace(line);
                }
                ui.add_space(8.0);
            });
    }
    ui.add_space(8.0);
}

fn paint_file_info(ui: &mut egui::Ui, res: &ParseResult) {
    let title = match res.file_type {
        FileType::H264 => "H.264/AVC",
        FileType::H265 => "H.265/HEVC",
        _ => "File",
    };
    ui.label(title);
    if let Some(ref s) = res.sps_info {
        ui.label(format!("Picture Size: {}x{}", s.width, s.height));
        ui.label(format!("Cropping Left: {}  Cropping Right: {}  Cropping Top: {}  Cropping Bottom: {}", s.crop_left, s.crop_right, s.crop_top, s.crop_bottom));
        ui.label("Video Format: YUV420 Luma bit: 8 Chroma bit: 8");
        let stream_str = match res.file_type {
            FileType::H265 => {
                let profile = match s.profile_idc {
                    1 => "Main Profile",
                    2 => "Main 10 Profile",
                    3 => "Main Still Picture",
                    _ => "Profile",
                };
                let level_str = level_idc_to_tier_level(s.level_idc);
                format!("Stream Type: {} @ Level {}({}) Tier Main", profile, level_str, s.level_idc)
            }
            _ => {
                let profile = match s.profile_idc {
                    66 => "Baseline",
                    77 => "Main",
                    88 => "Extended",
                    100 => "High",
                    _ => "Profile",
                };
                format!("Stream Type: {} Profile @ Level {}", profile, s.level_idc)
            }
        };
        ui.label(stream_str);
        if let Some(ref p) = res.pps_info {
            let enc = if p.entropy_coding_mode_flag { "CABAC" } else { "CAVLC" };
            ui.label(format!("Encoding Type: {}", enc));
        }
        if s.max_framerate > 0.0 {
            ui.label(format!("Max fps: {:.3}", s.max_framerate));
        }
        let frame_count = res.nalus.iter().filter(|n| {
            matches!(n.nal_type, NalUnitType::IdrSlice | NalUnitType::NonIdrSlice)
                || n.h265_nal_type == Some(0) || n.h265_nal_type == Some(1) || n.h265_nal_type == Some(18) || n.h265_nal_type == Some(19)
        }).count();
        ui.label(format!("Frame Count: {}", frame_count));
    } else {
        ui.weak("No SPS info (H.265 or unsupported)");
    }
}

/// HEVC level_idc to "4" style string (level_idc 120 -> 4.0, 90 -> 3.0, etc.).
fn level_idc_to_tier_level(level_idc: u8) -> String {
    let (major, minor) = match level_idc {
        30 => (1, 0),
        60 => (2, 0),
        63 => (2, 1),
        90 => (3, 0),
        93 => (3, 1),
        111 => (3, 2),
        120 => (4, 0),
        123 => (4, 1),
        126 => (4, 2),
        153 => (5, 0),
        156 => (5, 1),
        159 => (5, 2),
        162 => (5, 3),
        180 => (6, 0),
        183 => (6, 1),
        186 => (6, 2),
        189 => (6, 3),
        192 => (6, 4),
        _ => (0, 0),
    };
    if minor == 0 {
        format!("{}", major)
    } else {
        format!("{}.{}", major, minor)
    }
}

fn hex_dump(data: &[u8], bytes_per_line: usize) -> String {
    let mut out = String::new();
    for (i, chunk) in data.chunks(bytes_per_line).enumerate() {
        let offset = i * bytes_per_line;
        out.push_str(&format!("{:08x}  ", offset));
        for &b in chunk {
            out.push_str(&format!("{:02x} ", b));
        }
        let pad = bytes_per_line - chunk.len();
        for _ in 0..pad {
            out.push_str("   ");
        }
        out.push_str(" |");
        for &b in chunk {
            let c = if (0x20..0x7e).contains(&b) {
                b as char
            } else {
                '.'
            };
            out.push(c);
        }
        for _ in 0..pad {
            out.push(' ');
        }
        out.push_str("|\n");
    }
    out
}
