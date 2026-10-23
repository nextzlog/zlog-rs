//! The window of the RTTY console: the controls, the settings and messages, the
//! received text with its callsigns to click, the list of callsigns heard and the
//! line to send.

use egui::{Color32, RichText, Ui};
use zlog_core::Mode;
use zlog_ui::View;

use crate::terminal::is_callsign;
use crate::{audio, texts, RttyPanel, BAUDS, SCHEMES};

/// Width of the list of callsigns heard.
const CALLS_WIDTH: f32 = 120.0;

/// The text, background and accent colours of scheme `n`: those of the
/// application's theme for 0 ("follow the theme"), else zLog's `URttyOptions`
/// ones (normal, dark, green, amber) with an accent that reads on them.
fn scheme(ui: &Ui, n: usize) -> (Color32, Color32, Color32) {
    let rgb = Color32::from_rgb;
    let light = Color32::BLUE;
    let dark = rgb(0x78, 0xA0, 0xFF);
    match n {
        1 => (Color32::BLACK, Color32::WHITE, light),
        2 => (rgb(0xDC, 0xDC, 0xDC), rgb(0x1E, 0x1E, 0x1E), dark),
        3 => (rgb(0x40, 0xE0, 0x40), rgb(0x00, 0x10, 0x00), dark),
        4 => (rgb(0xFF, 0xB0, 0x30), rgb(0x00, 0x0B, 0x16), dark),
        _ => {
            let v = ui.visuals();
            (v.text_color(), v.extreme_bg_color, zlog_ui::palette::cq(ui))
        }
    }
}

impl RttyPanel {
    pub(crate) fn console_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.controls_ui(ui, view);
        egui::CollapsingHeader::new(texts().get(lang, "settings"))
            .id_salt("rtty-settings")
            .show(ui, |ui| self.settings_ui(ui, lang));
        egui::CollapsingHeader::new(texts().get(lang, "messages"))
            .id_salt("rtty-messages")
            .show(ui, |ui| self.messages_ui(ui, view));
        ui.separator();
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let height = (ui.available_height() - 3.0 * row).max(row * 3.0);
        ui.horizontal(|ui| {
            let width =
                (ui.available_width() - CALLS_WIDTH - ui.spacing().item_spacing.x).max(CALLS_WIDTH);
            ui.allocate_ui(egui::vec2(width, height), |ui| self.text_ui(ui));
            ui.allocate_ui(egui::vec2(CALLS_WIDTH, height), |ui| self.calls_ui(ui));
        });
        ui.separator();
        self.send_ui(ui, lang);
    }

    fn controls_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.receiving(), t("receive"))
                .clicked()
            {
                self.toggle_receive();
            }
            if let Some(level) = self.level() {
                ui.add(
                    egui::ProgressBar::new(level as f32)
                        .desired_width(80.0)
                        .text(t("level")),
                );
            }
            if ui.button(t("clear")).clicked() {
                self.terminal.clear_text();
            }
            if ui.button(t("clear_calls")).clicked() {
                self.terminal.calls.clear();
            }
            if view.mode != Mode::Rtty {
                ui.label(t("not_rtty"));
            }
            if !self.status.is_empty() {
                ui.colored_label(ui.visuals().warn_fg_color, &self.status);
            }
        });
    }

    fn device_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let default = t("default_device");
        let width = zlog_ui::form_field_width(ui, &[t("input"), t("output")]);
        for (label, id, value, list) in [
            (
                "input",
                "rtty-input",
                &mut self.settings.input,
                &self.inputs,
            ),
            (
                "output",
                "rtty-output",
                &mut self.settings.output,
                &self.outputs,
            ),
        ] {
            ui.label(t(label));
            let shown = if value.is_empty() {
                default.to_string()
            } else {
                value.clone()
            };
            egui::ComboBox::from_id_salt(id)
                .selected_text(shown)
                .width(width)
                .show_ui(ui, |ui| {
                    ui.selectable_value(value, String::new(), default);
                    for d in list {
                        ui.selectable_value(value, d.clone(), d);
                    }
                });
            ui.end_row();
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        egui::Grid::new("rtty-settings-grid")
            .num_columns(2)
            .show(ui, |ui| {
                self.device_ui(ui, lang);
                let s = &mut self.settings;
                ui.label(t("tones"));
                ui.horizontal(|ui| {
                    ui.label(t("mark"));
                    ui.add(
                        egui::DragValue::new(&mut s.mark)
                            .range(300..=3000)
                            .suffix(" Hz"),
                    );
                    ui.label(t("shift"));
                    ui.add(
                        egui::DragValue::new(&mut s.shift)
                            .range(50..=1000)
                            .suffix(" Hz"),
                    );
                    ui.label(t("baud"));
                    egui::ComboBox::from_id_salt("rtty-baud")
                        .selected_text(s.baud.to_string())
                        .show_ui(ui, |ui| {
                            for b in BAUDS {
                                ui.selectable_value(&mut s.baud, b, b.to_string());
                            }
                        });
                });
                ui.end_row();
                ui.label("");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.reverse, t("reverse"));
                    ui.checkbox(&mut s.uos, t("uos"));
                    ui.label(t("squelch"));
                    ui.add(egui::Slider::new(&mut s.squelch, 0.0..=1.0));
                });
                ui.end_row();
                ui.label(t("ptt_delay"));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut s.ptt, t("ptt"));
                    ui.add(egui::DragValue::new(&mut s.ptt_before_ms).range(0..=2000));
                    ui.add(egui::DragValue::new(&mut s.ptt_after_ms).range(0..=2000));
                    ui.label("ms");
                });
                ui.end_row();
                ui.label(t("colours"));
                ui.horizontal(|ui| {
                    for n in 0..SCHEMES {
                        let key = format!("scheme{n}");
                        ui.radio_value(&mut s.scheme, n, texts().get(lang, &key));
                    }
                });
                ui.end_row();
                ui.label(t("highlight"));
                let width = zlog_ui::form_field_width(ui, &[t("highlight")]);
                let edit =
                    egui::TextEdit::singleline(&mut s.highlight).hint_text(t("highlight.hint"));
                zlog_ui::sized_field(ui, edit, width);
                ui.end_row();
            });
        if ui.button(t("find_devices")).clicked() {
            self.inputs = audio::devices(true);
            self.outputs = audio::devices(false);
        }
    }

    fn messages_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let mut play = None;
        egui::Grid::new("rtty-messages-grid")
            .num_columns(2)
            .show(ui, |ui| {
                let width = zlog_ui::form_field_width(ui, &["F12"]);
                for n in 0..12 {
                    if ui.button(format!("F{}", n + 1)).clicked() {
                        play = Some(n);
                    }
                    let edit = egui::TextEdit::singleline(&mut self.settings.messages[n]);
                    zlog_ui::sized_field(ui, edit, width);
                    ui.end_row();
                }
            });
        if let Some(n) = play {
            self.play(n, view);
        }
    }

    /// The received text, a line to a row; a callsign in it is clicked into the
    /// entry row.
    fn text_ui(&mut self, ui: &mut Ui) {
        let (fg, bg, accent) = scheme(ui, self.settings.scheme);
        let mut picked = None;
        egui::Frame::new()
            .fill(bg)
            .inner_margin(4.0)
            .show(ui, |ui| {
                let row = ui.text_style_height(&egui::TextStyle::Monospace);
                let lines: Vec<&str> = self.terminal.all_lines().collect();
                egui::ScrollArea::vertical()
                    .id_salt("rtty-text")
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show_rows(ui, row, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (i, word) in line.split(' ').enumerate() {
                                    if i > 0 {
                                        ui.label(RichText::new(" ").monospace());
                                    }
                                    if is_callsign(word) {
                                        let text = RichText::new(word)
                                            .monospace()
                                            .color(accent)
                                            .underline();
                                        let r = ui.add(
                                            egui::Label::new(text).sense(egui::Sense::click()),
                                        );
                                        if r.clicked() {
                                            picked = Some(word.to_string());
                                        }
                                    } else if self.settings.highlighted(word) {
                                        ui.label(
                                            RichText::new(word).monospace().strong().color(accent),
                                        );
                                    } else {
                                        ui.label(RichText::new(word).monospace().color(fg));
                                    }
                                }
                            });
                        }
                    });
            });
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    /// The callsigns heard, the newest first.
    fn calls_ui(&mut self, ui: &mut Ui) {
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("rtty-calls")
            .auto_shrink(false)
            .show(ui, |ui| {
                for c in &self.terminal.calls {
                    if ui
                        .selectable_label(false, RichText::new(c).monospace())
                        .clicked()
                    {
                        picked = Some(c.clone());
                    }
                }
            });
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    /// The line to send, and what is being sent.
    fn send_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let busy = self.sender.busy();
        let (mut clicked, mut stop) = (false, false);
        let entered = zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t("send_text"));
            },
            |ui| {
                stop = ui.add_enabled(busy, egui::Button::new(t("stop"))).clicked();
                clicked = ui.button(t("send")).clicked();
            },
            |ui| {
                let r = ui
                    .add(egui::TextEdit::singleline(&mut self.typed).desired_width(f32::INFINITY));
                let entered = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if entered {
                    r.request_focus();
                }
                entered
            },
        );
        if clicked || entered {
            let text = std::mem::take(&mut self.typed);
            self.send(&text);
        }
        if stop {
            self.stop();
        }
        let state = if self.sender.busy() {
            format!("{} {}", t("sending"), self.sending)
        } else if self.receiving() {
            t("receiving").to_string()
        } else {
            t("idle").to_string()
        };
        ui.label(state);
    }
}
