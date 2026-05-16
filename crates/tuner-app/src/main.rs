//! egui-based GUI tuner.

mod audio;

use anyhow::Result;
use eframe::egui;
use tuner_core::{DetectorConfig, Pitch};

use audio::AudioSession;

const CENTS_RANGE: f32 = 50.0;
const IN_TUNE_CENTS: f32 = 5.0;
const MIN_DISPLAY_CONFIDENCE: f32 = 0.5;

fn main() -> Result<()> {
    let detector = DetectorConfig::default();
    let hop = detector.window_len / 4;
    let session = audio::start(detector, hop)?;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 320.0])
            .with_min_inner_size([360.0, 240.0])
            .with_title("tuner"),
        ..Default::default()
    };

    eframe::run_native(
        "tuner",
        native_options,
        Box::new(|_cc| Ok(Box::new(TunerApp::new(session)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

struct TunerApp {
    session: AudioSession,
    last: Option<Pitch>,
}

impl TunerApp {
    fn new(session: AudioSession) -> Self {
        Self {
            session,
            last: None,
        }
    }
}

impl eframe::App for TunerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(p) = self.session.handle.latest() {
            self.last = Some(p);
        }

        egui::Panel::top("top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.session.device_name);
                ui.separator();
                ui.label(format!("{:.1} kHz", self.session.sample_rate / 1000.0));
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let displayable = self.last.filter(|p| p.confidence >= MIN_DISPLAY_CONFIDENCE);

            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                let note_text = match displayable {
                    Some(p) => format!("{}{}", p.note.name, p.note.octave),
                    None => "—".to_string(),
                };
                ui.label(
                    egui::RichText::new(note_text)
                        .size(96.0)
                        .strong()
                        .color(note_color(displayable)),
                );
            });

            ui.add_space(8.0);
            draw_meter(ui, displayable.map(|p| p.cents));

            ui.add_space(12.0);
            ui.vertical_centered(|ui| match displayable {
                Some(p) => {
                    ui.label(format!("{:+.1} cents", p.cents));
                    ui.label(format!("{:.2} Hz   conf {:.2}", p.hz, p.confidence));
                }
                None => {
                    ui.label("listening…");
                }
            });
        });

        ui.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

fn note_color(p: Option<Pitch>) -> egui::Color32 {
    match p {
        Some(p) if p.cents.abs() <= IN_TUNE_CENTS => egui::Color32::from_rgb(80, 220, 120),
        Some(_) => egui::Color32::from_rgb(230, 230, 230),
        None => egui::Color32::from_rgb(120, 120, 120),
    }
}

fn draw_meter(ui: &mut egui::Ui, cents: Option<f32>) {
    let desired = egui::vec2(ui.available_width().min(440.0), 60.0);
    let (rect, _resp) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    let mid_x = rect.center().x;
    let baseline_y = rect.center().y;
    let half_w = rect.width() * 0.45;

    painter.rect_filled(
        egui::Rect::from_center_size(egui::pos2(mid_x, baseline_y), egui::vec2(half_w * 2.0, 6.0)),
        2.0,
        egui::Color32::from_gray(60),
    );

    let band_w = (IN_TUNE_CENTS / CENTS_RANGE) * half_w;
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(mid_x, baseline_y),
            egui::vec2(band_w * 2.0, 10.0),
        ),
        3.0,
        egui::Color32::from_rgb(40, 90, 50),
    );

    for c in (-50..=50).step_by(10) {
        let x = mid_x + (c as f32 / CENTS_RANGE) * half_w;
        let h = if c == 0 { 18.0 } else { 10.0 };
        painter.line_segment(
            [egui::pos2(x, baseline_y - h), egui::pos2(x, baseline_y + h)],
            egui::Stroke::new(
                if c == 0 { 2.0 } else { 1.0 },
                egui::Color32::from_gray(140),
            ),
        );
    }

    if let Some(cents) = cents {
        let clamped = cents.clamp(-CENTS_RANGE, CENTS_RANGE);
        let x = mid_x + (clamped / CENTS_RANGE) * half_w;
        let color = if cents.abs() <= IN_TUNE_CENTS {
            egui::Color32::from_rgb(80, 220, 120)
        } else {
            egui::Color32::from_rgb(240, 180, 60)
        };
        painter.line_segment(
            [
                egui::pos2(x, baseline_y - 24.0),
                egui::pos2(x, baseline_y + 24.0),
            ],
            egui::Stroke::new(3.0, color),
        );
        painter.circle_filled(egui::pos2(x, baseline_y), 6.0, color);
    }
}
