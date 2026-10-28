//! Search and a bounded list shared by the database and schema breadcrumbs.

#[derive(Default)]
pub struct LocationPicker {
    query: String,
    selected: usize,
    focused: bool,
}

fn matches_name(name: &str, query: &str) -> bool {
    super::workspace::fuzzy(&name.to_lowercase(), &query.to_lowercase())
}

impl LocationPicker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, names: &[String], current: &str) -> Option<String> {
        ui.set_width(320.0_f32.min(ui.ctx().content_rect().width() - 24.0));
        let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if down || up {
            // egui may already have scheduled spatial focus movement before we consumed the key.
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .event_filter(egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                })
                .hint_text("Search…")
                .desired_width(f32::INFINITY),
        );
        if !self.focused {
            search.request_focus();
            self.focused = true;
        }
        if search.changed() {
            self.selected = 0;
        }
        let filtered: Vec<_> = names
            .iter()
            .filter(|name| matches_name(name, &self.query))
            .collect();
        self.selected = self.selected.min(filtered.len().saturating_sub(1));
        let mut reveal = search.changed();
        if down {
            self.selected = (self.selected + 1).min(filtered.len().saturating_sub(1));
            reveal = true;
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
            reveal = true;
        }
        let mut chosen = None;
        if enter {
            chosen = filtered.get(self.selected).map(|name| (*name).clone());
        }
        ui.separator();
        let height = (ui.ctx().content_rect().height() - 160.0).clamp(80.0, 320.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                if filtered.is_empty() {
                    ui.label(super::dim("No matches"));
                }
                for (i, name) in filtered.iter().enumerate() {
                    let label = if name.as_str() == current {
                        format!("✓ {name}")
                    } else {
                        (*name).clone()
                    };
                    let response = ui
                        .add(egui::Button::selectable(i == self.selected, label).truncate())
                        .on_hover_text(name.as_str());
                    if reveal && i == self.selected {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        chosen = Some((*name).clone());
                    }
                }
            });
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_all_names_and_bounds_long_lists() {
        let ctx = egui::Context::default();
        let names: Vec<String> = (0..500).map(|i| format!("database_{i:03}")).collect();
        let mut picker = LocationPicker::default();
        let mut height = 0.0;
        let mut chosen = None;
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(420.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    chosen = picker.show(ui, &names, "database_000");
                    height = ui.min_rect().height();
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        frame(vec![egui::Event::Text("499".into())]);
        frame(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(chosen.as_deref(), Some("database_499"));
        assert!(height < 400.0, "picker height: {height}");
    }

    #[test]
    fn case_insensitive_search_and_empty_results() {
        assert!(matches_name("Merchant_STAGING", "merchant"));
        assert!(matches_name("Merchant_STAGING", "MSTG"));
        assert!(!matches_name("Merchant_STAGING", "xyz"));
    }
}
