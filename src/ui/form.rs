//! Flat forms: a bordered group of 40 px rows on the ground colour, hairlines between rows,
//! labels in one column so every value starts on the same line. Used by the connection dialog
//! and Settings.

use super::*;
use egui::{pos2, vec2, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, Ui};

/// A titled group. `label_w` is the label column; `None` puts the label left and the control right.
pub struct Group<'a> {
    pub ui: &'a mut Ui,
    label_w: Option<f32>,
    rows: usize,
}

pub fn group<R>(ui: &mut Ui, title: &str, label_w: Option<f32>, add: impl FnOnce(&mut Group) -> R) -> R {
    group_with_note(ui, title, "", label_w, add)
}

pub fn group_with_note<R>(ui: &mut Ui, title: &str, note: &str, label_w: Option<f32>, add: impl FnOnce(&mut Group) -> R) -> R {
    let pal = p();
    if !title.is_empty() {
        ui.label(RichText::new(title).font(medium(12.0)).color(pal.text_dim));
        ui.add_space(8.0);
    }
    // A soft grey group, 12 px corners, no border: the rows inside stay flat.
    let out = egui::Frame::new().fill(pal.control()).corner_radius(CornerRadius::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut g = Group { ui, label_w, rows: 0 };
        add(&mut g)
    });
    if !note.is_empty() {
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(note).size(12.0).color(pal.text_dim)).wrap());
    }
    ui.add_space(24.0);
    out.inner
}

impl Group<'_> {
    /// A raw row: `add` gets the area right of the label (or the whole row without a label).
    pub fn row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.row_h(label, 40.0, add)
    }

    pub fn row_h<R>(&mut self, label: &str, min_h: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
        let pal = p();
        let ui = &mut *self.ui;
        let first = self.rows == 0;
        self.rows += 1;
        let bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().top();
        let full = ui.available_width();
        let label_w = self.label_w;
        let inner = ui.allocate_ui_with_layout(vec2(full, min_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(full, min_h));
            ui.spacing_mut().item_spacing.x = 0.0;
            match label_w {
                Some(w) => {
                    let (r, _) = ui.allocate_exact_size(vec2(w, min_h), Sense::hover());
                    let text = super::home::ellipsize_to(ui.painter(), label, egui::FontId::proportional(13.0), w - 24.0);
                    ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), pal.text_dim);
                    ui.allocate_ui_with_layout(vec2(ui.available_width(), min_h), egui::Layout::left_to_right(egui::Align::Center), |ui| add(ui)).inner
                }
                None => {
                    ui.add_space(12.0);
                    if !label.is_empty() {
                        ui.label(RichText::new(label).color(pal.text));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        add(ui)
                    }).inner
                }
            }
        });
        let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().left() + full, inner.response.rect.bottom()));
        self.finish_row(rect, bg, first);
        inner.inner
    }

    fn finish_row(&mut self, rect: Rect, bg: egui::layers::ShapeIdx, first: bool) {
        let pal = p();
        let ui = &mut *self.ui;
        let focused = ui.ctx().memory(|m| m.focused()).and_then(|id| ui.ctx().read_response(id)).map(|r| rect.contains(r.rect.center())).unwrap_or(false);
        let hovered = ui.rect_contains_pointer(rect);
        let id = ui.scope_id().with(("form-row", rect.top() as i32));
        let t = ui.ctx().animate_bool_with_time(id, hovered || focused, motion::FAST as f32);
        let mut shapes = Vec::new();
        if t > 0.0 {
            shapes.push(egui::Shape::rect_filled(rect, CornerRadius::ZERO, pal.sidebar.gamma_multiply(t)));
        }
        if focused {
            shapes.push(egui::Shape::rect_stroke(rect.shrink(0.75), CornerRadius::same(5), Stroke::new(1.5, pal.grid_selection_border), egui::StrokeKind::Inside));
        }
        if !first {
            shapes.push(egui::Shape::hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, line())));
        }
        ui.painter().set(bg, egui::Shape::Vec(shapes));
    }

    /// Single-line text value. `mono` for hosts, users and paths.
    pub fn text(&mut self, label: &str, value: &mut String, hint: &str, mono: bool) -> Response {
        self.row(label, |ui| field_edit(ui, value, hint, mono, false, ui.available_width() - 12.0))
    }

    /// A secret with a reveal toggle.
    pub fn secret(&mut self, label: &str, value: &mut String, hint: &str) -> Response {
        let id = self.ui.scope_id().with(("reveal", label));
        let mut shown = self.ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let r = self.row(label, |ui| {
            let r = field_edit(ui, value, hint, true, !shown, ui.available_width() - 36.0);
            if mini_icon(ui, Icon::Eye, if shown { "Hide" } else { "Show" }).clicked() {
                shown = !shown;
            }
            r
        });
        self.ui.ctx().data_mut(|d| d.insert_temp(id, shown));
        r
    }

    /// Text value with a trailing Browse… button.
    pub fn path(&mut self, label: &str, value: &mut String, hint: &str) -> bool {
        self.row(label, |ui| {
            field_edit(ui, value, hint, true, false, ui.available_width() - 92.0);
            ui.add_space(8.0);
            mini(ui, "Browse…").clicked()
        })
    }

    /// Host and port on one row.
    pub fn host_port(&mut self, label: &str, host: &mut String, hint: &str, port: &mut u16) {
        let pal = p();
        self.row(label, |ui| {
            field_edit(ui, host, hint, true, false, (ui.available_width() - 132.0).max(60.0));
            let (r, _) = ui.allocate_exact_size(vec2(56.0, 40.0), Sense::hover());
            ui.painter().vline(r.left() + 0.5, r.y_range(), Stroke::new(1.0, line()));
            ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, "Port", egui::FontId::proportional(13.0), pal.text_dim);
            let mut s = port.to_string();
            if field_edit(ui, &mut s, "", true, false, 64.0).changed() {
                if let Ok(v) = s.trim().parse::<u16>() {
                    *port = v;
                } else if s.trim().is_empty() {
                    *port = 0;
                }
            }
        });
    }

    pub fn seg<T: PartialEq + Copy>(&mut self, label: &str, value: &mut T, options: &[(T, &str)]) -> bool {
        let fill = self.label_w.is_some();
        self.row(label, |ui| {
            let w = if fill { ui.available_width() - 4.0 } else { 0.0 };
            seg_control(ui, value, options, w)
        })
    }

    /// A choice from a short list, shown as a menu under the row.
    pub fn menu<T: PartialEq + Clone>(&mut self, label: &str, value: &mut T, options: &[(T, String, Option<Color32>)]) -> bool {
        let pal = p();
        let mut changed = false;
        self.row(label, |ui| {
            let (text, dot) = options.iter().find(|o| o.0 == *value).map(|o| (o.1.clone(), o.2)).unwrap_or_default();
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 39.0), Sense::click());
            let mut x = r.left();
            if let Some(c) = dot {
                ui.painter().circle_filled(pos2(x + 4.0, r.center().y), 4.0, c);
                x += 16.0;
            }
            ui.painter().text(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), pal.text);
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
            let t = ui.ctx().animate_bool_with_time(resp.id.with("chev"), open, motion::SETTLE as f32);
            paint_chevron_down(ui.painter(), Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2(12.0, 12.0)), t, pal.text_faint);
            egui::Popup::menu(&resp).width(r.width()).show(|ui| {
                for (v, l, c) in options {
                    let on = *v == *value;
                    let (rr, rs) = ui.allocate_exact_size(vec2(ui.available_width().max(r.width() - 16.0), 30.0), Sense::click());
                    if rs.hovered() {
                        ui.painter().rect_filled(rr, CornerRadius::same(4), pal.surface_hover);
                    }
                    let mut x = rr.left() + 8.0;
                    if let Some(c) = c {
                        ui.painter().circle_filled(pos2(x + 4.0, rr.center().y), 4.0, *c);
                        x += 16.0;
                    }
                    ui.painter().text(pos2(x, rr.center().y), egui::Align2::LEFT_CENTER, l, egui::FontId::proportional(13.0), pal.text);
                    if on {
                        paint_icon(ui.painter(), Rect::from_center_size(pos2(rr.right() - 14.0, rr.center().y), vec2(12.0, 12.0)), Icon::Check, pal.grid_selection_border);
                    }
                    if rs.clicked() {
                        *value = v.clone();
                        changed = true;
                        ui.close();
                    }
                }
            });
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        });
        changed
    }

    /// Label and a short description on the left, a switch on the right.
    pub fn switch(&mut self, label: &str, desc: &str, value: &mut bool) -> bool {
        let pal = p();
        let mut changed = false;
        let ui = &mut *self.ui;
        let first = self.rows == 0;
        self.rows += 1;
        let bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().top();
        let full = ui.available_width();
        let h = if desc.is_empty() { 40.0 } else { 52.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2(full, h), Sense::click());
        if desc.is_empty() {
            ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), pal.text);
        } else {
            ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 8.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(13.0), pal.text);
            ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 28.0), egui::Align2::LEFT_TOP, desc, egui::FontId::proportional(12.0), pal.text_dim);
        }
        let sw = Rect::from_center_size(pos2(rect.right() - 12.0 - 16.0, rect.center().y), vec2(32.0, 20.0));
        paint_switch(ui, sw, resp.id, *value);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *value, label));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *value = !*value;
            changed = true;
        }
        let rect = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        self.finish_row(rect, bg, first);
        changed
    }

    /// A radio row: circle, bold name and a description.
    pub fn radio(&mut self, label: &str, desc: &str, selected: bool) -> bool {
        let pal = p();
        let ui = &mut *self.ui;
        let first = self.rows == 0;
        self.rows += 1;
        let bg = ui.painter().add(egui::Shape::Noop);
        let full = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(full, 52.0), Sense::click());
        let c = pos2(rect.left() + 16.0 + (if self.label_w.is_some() { 0.0 } else { 4.0 }), rect.center().y);
        let t = ui.ctx().animate_bool_with_time(resp.id.with("radio"), selected, motion::SETTLE as f32);
        ui.painter().circle_stroke(c, 7.5, Stroke::new(1.0, line2().lerp_to_gamma(pal.text, t)));
        if t > 0.0 {
            ui.painter().circle_filled(c, 4.0 * motion::ease_out(t), pal.text);
        }
        let x = rect.left() + 36.0;
        ui.painter().text(pos2(x, rect.top() + 8.0), egui::Align2::LEFT_TOP, label, medium(13.0), pal.text);
        ui.painter().text(pos2(x, rect.top() + 28.0), egui::Align2::LEFT_TOP, desc, egui::FontId::proportional(12.0), pal.text_dim);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, label));
        let clicked = resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        self.finish_row(rect, bg, first);
        clicked
    }

    /// Multi-line value under its label.
    pub fn area(&mut self, label: &str, value: &mut String, hint: &str) {
        let pal = p();
        let ui = &mut *self.ui;
        let first = self.rows == 0;
        self.rows += 1;
        let bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().top();
        let full = ui.available_width();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(label).color(pal.text_dim));
        });
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.add(egui::TextEdit::multiline(value).frame(egui::Frame::NONE).font(egui::FontId::monospace(13.0)).desired_rows(4).desired_width(full - 24.0).hint_text(RichText::new(hint).color(pal.text_faint)));
        });
        ui.add_space(8.0);
        let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().left() + full, ui.cursor().top()));
        self.finish_row(rect, bg, first);
    }

    /// Swatch picker: an "auto" chip then each colour.
    pub fn swatches(&mut self, label: &str, value: &mut Option<[u8; 3]>, colors: &[[u8; 3]]) {
        let pal = p();
        self.row(label, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let chip = |ui: &mut Ui, on: bool| -> (Rect, Response) {
                let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(4), pal.surface_hover);
                }
                if on {
                    ui.painter().circle_stroke(r.center(), 10.75, Stroke::new(1.5, pal.text));
                }
                (r, resp.on_hover_cursor(egui::CursorIcon::PointingHand))
            };
            let (r, resp) = chip(ui, value.is_none());
            ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.0, line2()));
            ui.painter().line_segment([r.center() + vec2(-5.0, 5.0), r.center() + vec2(5.0, -5.0)], Stroke::new(1.0, pal.text_faint));
            if resp.on_hover_text("Use the tag or driver colour").clicked() {
                *value = None;
            }
            for c in colors {
                let (r, resp) = chip(ui, *value == Some(*c));
                ui.painter().circle_filled(r.center(), 8.0, crate::theme::rgb(*c));
                if resp.clicked() {
                    *value = Some(*c);
                }
            }
        });
    }

    /// A number with − and + on the right. Returns the new value when it changes.
    pub fn step(&mut self, label: &str, value: f64, delta: f64, min: f64, max: f64, unit: &str) -> Option<f64> {
        let pal = p();
        let mut out = None;
        self.row(label, |ui| {
            ui.add_space(4.0);
            if mini_text(ui, "+", &format!("Increase {label}")).clicked() && value < max {
                out = Some((value + delta).min(max));
            }
            let text = if value.fract() == 0.0 { format!("{}{unit}", value as i64) } else { format!("{value:.1}{unit}") };
            let (r, _) = ui.allocate_exact_size(vec2(56.0, 28.0), Sense::hover());
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(13.0), pal.text);
            if mini_text(ui, "−", &format!("Decrease {label}")).clicked() && value > min {
                out = Some((value - delta).max(min));
            }
        });
        out
    }

    /// Label with a description and a secondary button on the right.
    pub fn button(&mut self, label: &str, desc: &str, text: &str) -> bool {
        let pal = p();
        let ui = &mut *self.ui;
        let first = self.rows == 0;
        self.rows += 1;
        let bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().top();
        let full = ui.available_width();
        let h = if desc.is_empty() { 40.0 } else { 52.0 };
        let mut clicked = false;
        let inner = ui.allocate_ui_with_layout(vec2(full, h), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(full, h));
            ui.add_space(8.0);
            clicked = btn(ui, text, BtnKind::Secondary).clicked();
        });
        let rect = Rect::from_min_max(pos2(inner.response.rect.left(), top), pos2(inner.response.rect.left() + full, inner.response.rect.bottom()));
        if desc.is_empty() {
            ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), pal.text);
        } else {
            ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 8.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(13.0), pal.text);
            ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 28.0), egui::Align2::LEFT_TOP, desc, egui::FontId::proportional(12.0), pal.text_dim);
        }
        self.finish_row(rect, bg, first);
        clicked
    }

    /// Key caps on the right, e.g. a shortcut.
    pub fn keys(&mut self, label: &str, keys: &[&str]) {
        let pal = p();
        self.row(label, |ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            for k in keys.iter().rev() {
                let g = ui.painter().layout_no_wrap(k.to_string(), egui::FontId::monospace(12.0), pal.text_dim);
                let (r, _) = ui.allocate_exact_size(vec2((g.size().x + 8.0).max(20.0), 20.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(5), pal.control());
                ui.painter().galley(r.center() - g.size() / 2.0, g, pal.text_dim);
            }
        });
    }
}

/// Frameless field text, 39 px tall.
pub fn field_edit(ui: &mut Ui, value: &mut String, hint: &str, mono: bool, password: bool, width: f32) -> Response {
    let pal = p();
    let font = if mono { egui::FontId::monospace(13.0) } else { egui::FontId::proportional(13.0) };
    ui.add_sized(
        vec2(width.max(20.0), 39.0),
        line_edit(value).frame(egui::Frame::NONE).font(font).password(password).hint_text(RichText::new(hint).color(pal.text_faint)),
    )
}

/// Small text button inside a row (28 px, 4 px corners).
pub fn mini(ui: &mut Ui, text: &str) -> Response {
    let pal = p();
    let g = ui.painter().layout_no_wrap(text.to_string(), medium(13.0), pal.text_dim);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, 28.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), motion::FAST as f32);
    if t > 0.0 {
        ui.painter().rect_filled(r, CornerRadius::same(8), pal.control_hover().gamma_multiply(t));
    }
    ui.painter().galley(r.center() - g.size() / 2.0, g, mix(pal.text_dim, pal.text, t));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn mini_text(ui: &mut Ui, text: &str, tip: &str) -> Response {
    let pal = p();
    let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), motion::FAST as f32);
    if t > 0.0 {
        ui.painter().rect_filled(r, CornerRadius::same(8), pal.control_hover().gamma_multiply(t));
    }
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(14.0), mix(pal.text_dim, pal.text, t));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn mini_icon(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let pal = p();
    let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), motion::FAST as f32);
    if t > 0.0 {
        ui.painter().rect_filled(r, CornerRadius::same(8), pal.control_hover().gamma_multiply(t));
    }
    paint_icon(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, mix(pal.text_dim, pal.text, t));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 32 × 20 switch in the macOS shape: a soft grey track that turns green when on, a white knob.
pub fn paint_switch(ui: &Ui, r: Rect, id: egui::Id, on: bool) {
    let pal = p();
    let t = motion::ease_out(ui.ctx().animate_bool_with_time(id.with("switch"), on, motion::SETTLE as f32));
    let off = pal.control_hover();
    let track = if t <= 0.0 { off } else { off.lerp_to_gamma(pal.success, t) };
    ui.painter().rect_filled(r, CornerRadius::same(255), track);
    let knob = pos2(r.left() + 10.0 + 12.0 * t, r.center().y);
    ui.painter().circle_filled(knob + vec2(0.0, 0.5), 8.0, Color32::from_black_alpha(40));
    ui.painter().circle_filled(knob, 8.0, Color32::WHITE);
}

/// A switch on its own, for toolbars ("Auto refresh").
pub fn switch(ui: &mut Ui, value: &mut bool, label: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 20.0), Sense::click());
    paint_switch(ui, r, resp.id, *value);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *value, label));
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// Segmented control: a s2 well, equal segments, and a thumb that slides to the choice.
/// `width` 0 sizes it to the labels.
pub fn seg_control<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], width: f32) -> bool {
    let pal = p();
    let font = medium(13.0);
    let widest = options.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), font.clone(), pal.text).size().x).fold(0.0, f32::max);
    let n = options.len().max(1) as f32;
    let w = if width > 0.0 { width } else { (widest + 20.0) * n + 4.0 };
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(255), pal.control());
    let seg_w = (r.width() - 4.0) / n;
    let idx = options.iter().position(|(v, _)| v == value).unwrap_or(0) as f32;
    let id = ui.scope_id().with(("seg", r.left() as i32, r.top() as i32));
    let x = motion::glide(ui.ctx(), id, idx, motion::SETTLE);
    let thumb = Rect::from_min_size(pos2(r.left() + 2.0 + x * seg_w, r.top() + 2.0), vec2(seg_w, 26.0));
    paint_thumb(ui.painter(), thumb);
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.left() + 2.0 + i as f32 * seg_w, r.top() + 2.0), vec2(seg_w, 26.0));
        let resp = ui.interact(cell, id.with(i), Sense::click());
        let on = *v == *value;
        let t = ui.ctx().animate_bool_with_time(resp.id, on || resp.hovered(), motion::FAST as f32);
        ui.painter().text(cell.center(), egui::Align2::CENTER_CENTER, *label, font.clone(), mix(pal.text_dim, pal.text, t));
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, *label));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// Chevron pointing down that turns to point up as `t` goes to 1.
pub fn paint_chevron_down(painter: &egui::Painter, r: Rect, t: f32, color: Color32) {
    let a = t * std::f32::consts::PI;
    let rot = |x: f32, y: f32| {
        let (s, c) = a.sin_cos();
        r.center() + vec2(x * c - y * s, x * s + y * c) * (r.width() / 12.0)
    };
    painter.add(egui::Shape::line(vec![rot(-3.5, -1.5), rot(0.0, 2.0), rot(3.5, -1.5)], Stroke::new(1.5, color)));
}

/// Underline tabs: fixed-width labels with an ink bar that slides under the current one.
pub fn underline_tabs<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], tab_w: f32) -> bool {
    let pal = p();
    let full = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(full, 36.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, line()));
    let idx = options.iter().position(|(v, _)| v == value).unwrap_or(0) as f32;
    let id = ui.scope_id().with("underline-tabs");
    let x = motion::glide(ui.ctx(), id, idx, motion::SETTLE);
    let left = r.left() + 16.0;
    ui.painter().rect_filled(Rect::from_min_size(pos2(left + x * tab_w, r.bottom() - 2.0), vec2(tab_w, 2.0)), CornerRadius::ZERO, pal.text);
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(left + i as f32 * tab_w, r.top()), vec2(tab_w, 36.0));
        let resp = ui.interact(cell, id.with(i), Sense::click());
        let on = *v == *value;
        let t = ui.ctx().animate_bool_with_time(resp.id, on || resp.hovered(), motion::FAST as f32);
        ui.painter().text(cell.center(), egui::Align2::CENTER_CENTER, *label, medium(13.0), mix(pal.text_dim, pal.text, t));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// The dialog shell: a solid sheet with 16 px corners, a hairline and a soft shadow; rises 8 px
/// and fades in. Dialogs stay solid, never glass.
pub fn dialog_frame() -> egui::Frame {
    let pal = p();
    egui::Frame::new()
        .fill(pal.sidebar)
        .stroke(Stroke::new(1.0, line()))
        .corner_radius(CornerRadius::same(16))
        .shadow(egui::epaint::Shadow { offset: [0, 16], blur: 48, spread: 0, color: if pal.dark { Color32::from_black_alpha(140) } else { Color32::from_rgba_unmultiplied(60, 50, 30, 56) } })
}

/// Shows a centred dialog of `size` with the design's entrance. Returns (inner, backdrop clicked).
pub fn dialog<R>(ctx: &egui::Context, id: &str, size: egui::Vec2, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let screen = ctx.content_rect();
    let size = vec2(size.x.min(screen.width() - 32.0), size.y.min(screen.height() - 48.0));
    let t = motion::ease_out(motion::entered(ctx, egui::Id::unique(id), motion::ENTER));
    // Dim the app behind.
    let backdrop = egui::Area::new(egui::Id::unique((id, "backdrop"))).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha((150.0 * t) as u8));
        resp.clicked()
    });
    let pos = screen.center() - size / 2.0 + vec2(0.0, 8.0 * (1.0 - t));
    let inner = egui::Area::new(egui::Id::unique(id)).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        ui.set_opacity(t);
        dialog_frame().show(ui, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            add(ui)
        }).inner
    });
    (inner.inner, backdrop.inner)
}

/// The chosen segment: a brighter pill. In the light theme it is white with a faint edge, like the
/// macOS segmented thumb.
fn paint_thumb(painter: &egui::Painter, thumb: Rect) {
    let pal = p();
    if !pal.dark {
        painter.rect_filled(thumb.translate(vec2(0.0, 0.5)), CornerRadius::same(255), Color32::from_black_alpha(22));
    }
    painter.rect_filled(thumb, CornerRadius::same(255), pal.control_on());
}

/// The sidebar's segmented control: a soft pill track with a brighter sliding thumb.
pub fn seg_well<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], width: f32) -> bool {
    let pal = p();
    let n = options.len().max(1) as f32;
    let width = if width > 0.0 {
        width
    } else {
        let widest = options.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), medium(13.0), pal.text).size().x).fold(0.0, f32::max);
        (widest + 20.0) * n + 4.0
    };
    let (r, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(255), pal.control());
    let seg_w = (r.width() - 4.0) / n;
    let idx = options.iter().position(|(v, _)| v == value).unwrap_or(0) as f32;
    let id = ui.scope_id().with(("seg-well", r.left() as i32, r.top() as i32));
    let x = motion::glide(ui.ctx(), id, idx, motion::SETTLE);
    paint_thumb(ui.painter(), Rect::from_min_size(pos2(r.left() + 2.0 + x * seg_w, r.top() + 3.0), vec2(seg_w, 26.0)));
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.left() + 2.0 + i as f32 * seg_w, r.top() + 3.0), vec2(seg_w, 26.0));
        let resp = ui.interact(cell, id.with(i), Sense::click());
        let on = *v == *value;
        let t = ui.ctx().animate_bool_with_time(resp.id, on || resp.hovered(), motion::FAST as f32);
        ui.painter().text(cell.center(), egui::Align2::CENTER_CENTER, *label, medium(13.0), mix(pal.text_dim, pal.text, t));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// A 32 px input on the ground colour with a leading icon (filter, search).
pub fn field(ui: &mut Ui, text: &mut String, hint: &str, icon: Icon, width: f32) -> Response {
    let pal = p();
    let (r, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let id = ui.scope_id().with(("field", hint));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let t = ui.ctx().animate_bool_with_time(id.with("focus"), focused, motion::FAST as f32);
    ui.painter().rect_filled(r, CornerRadius::same(255), pal.control());
    if t > 0.0 {
        ui.painter().rect_stroke(r, CornerRadius::same(255), Stroke::new(1.0, pal.text_dim.gamma_multiply(t)), egui::StrokeKind::Inside);
    }
    paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(16.0, 16.0)), icon, pal.text_faint);
    let inner = Rect::from_min_max(pos2(r.left() + 34.0, r.top()), pos2(r.right() - 8.0, r.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add(line_edit(text).id(id).frame(egui::Frame::NONE).hint_text(RichText::new(hint).color(pal.text_faint)).desired_width(inner.width()))
}

/// A roomy search field with a soft focus fill, without a text-edit outline.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, id: egui::Id, width: f32) -> Response {
    let pal = p();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    ui.painter().rect_filled(rect, CornerRadius::same(255), if focused { pal.control_hover() } else { pal.control() });
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)), Icon::Search, pal.text_dim);
    let inner = Rect::from_min_max(pos2(rect.left() + 36.0, rect.top()), pos2(rect.right() - 12.0, rect.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add(line_edit(text).id(id).frame(egui::Frame::NONE)
        .hint_text(RichText::new(hint).color(pal.text_faint)).desired_width(inner.width()))
        .on_hover_text(hint)
}

/// One 30px menu row: a fixed check column, then the label. Hover or `highlighted` (keyboard)
/// fills it; long labels end in "…" and show in full on hover.
pub fn menu_row(ui: &mut Ui, label: &str, checked: bool, highlighted: bool) -> Response {
    let pal = p();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("hover"), highlighted || resp.hovered(), motion::FAST as f32);
    if h > 0.0 {
        ui.painter().rect_filled(r, CornerRadius::same(8), pal.control_hover().gamma_multiply(h));
    }
    if checked {
        paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0)), Icon::Check, pal.text);
    }
    let font = egui::FontId::proportional(13.0);
    let shown = super::home::ellipsize_to(ui.painter(), label, font.clone(), r.width() - 28.0 - 10.0);
    let truncated = shown != label;
    let color = if checked || highlighted || resp.hovered() { pal.text } else { pal.text_dim };
    ui.painter().text(pos2(r.left() + 28.0, r.center().y), egui::Align2::LEFT_CENTER, shown, font, color);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if truncated { resp.on_hover_text(label) } else { resp }
}

/// A hairline between groups of menu rows.
pub fn menu_rule(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, line()));
}
