//! Color themes. Keys follow the theme JSON format (`AppContentBackground`, `EditorKeywordText`, …)
//! so their theme files import as-is; `Strata*` keys add what Strata draws beyond that.

use egui::{Color32, CornerRadius, Stroke, Visuals};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub name: String,
    pub dark: bool,
    pub window: Color32,
    pub content: Color32,
    pub sidebar: Color32,
    pub rail: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub tab_selected: Color32,
    pub grid_header: Color32,
    pub grid_header_text: Color32,
    pub grid_even: Color32,
    pub grid_odd: Color32,
    pub grid_text: Color32,
    pub grid_number: Color32,
    pub grid_placeholder: Color32,
    pub grid_separator: Color32,
    pub grid_selection: Color32,
    pub grid_selection_border: Color32,
    pub modified: Color32,
    pub inserted: Color32,
    pub deleted: Color32,
    pub editor_bg: Color32,
    pub editor_text: Color32,
    pub editor_keyword: Color32,
    pub editor_function: Color32,
    pub editor_string: Color32,
    pub editor_dquote: Color32,
    pub editor_backtick: Color32,
    pub editor_number: Color32,
    pub editor_comment: Color32,
    pub editor_current_query: Color32,
    pub editor_selection: Color32,
    pub chart: [Color32; 5],
    /// Window ground behind the floating panels (translucent when the OS blurs the window).
    pub ground: Color32,
    /// Floating chrome: rail, command bar, explorer, inspector, docks.
    pub glass: Color32,
    pub glass_stroke: Color32,
    /// Secondary accent for links, filters, foreign keys and selection.
    pub accent2: Color32,
}

pub fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        6 => Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        8 => Some(Color32::from_rgba_unmultiplied((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)),
        _ => None,
    }
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn luminance(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

impl Palette {
    /// Builds a palette from theme JSON keys over a base palette.
    pub fn from_keys(name: &str, base: &Palette, keys: &BTreeMap<String, String>) -> Palette {
        let mut p = base.clone();
        p.name = name.to_string();
        let get = |k: &str| keys.get(k).and_then(|v| hex(v));
        macro_rules! set {
            ($field:ident, $($key:literal),+) => {
                $( if let Some(c) = get($key) { p.$field = c; } )+
            };
        }
        set!(window, "AppWindowBackground");
        set!(content, "AppContentBackground");
        set!(sidebar, "AppSidebarBackground");
        set!(surface, "AppControlBackground");
        set!(border, "AppControlBorder");
        set!(separator, "AppSeparator");
        set!(tab_selected, "AppTabSelectedBackground");
        set!(surface_hover, "AppTabHoverBackground");
        set!(text, "AppQueryMessageText", "GridText");
        set!(text_dim, "AppTabUnselectedText");
        set!(success, "AppNotificationSuccessBackground");
        set!(warning, "AppNotificationWarningBackground");
        set!(danger, "AppNotificationErrorBackground");
        set!(grid_header, "GridHeaderBackground");
        set!(grid_header_text, "GridHeaderText");
        set!(grid_even, "GridEvenRowBackground");
        set!(grid_odd, "GridOddRowBackground");
        set!(grid_text, "GridText");
        set!(grid_number, "GridNumberText");
        set!(grid_placeholder, "GridPlaceholderText");
        set!(grid_separator, "GridSeparator");
        set!(grid_selection_border, "GridCellSelectionBorder");
        set!(modified, "ContentModifiedBackground");
        set!(inserted, "ContentInsertedBackground");
        set!(deleted, "ContentDeletedRowBackground");
        set!(editor_text, "EditorText");
        set!(editor_keyword, "EditorKeywordText");
        set!(editor_function, "EditorFunctionText");
        set!(editor_string, "EditorSingleQuotedText");
        set!(editor_dquote, "EditorDoubleQuotedText");
        set!(editor_backtick, "EditorBacktickQuotedText");
        set!(editor_number, "EditorNumberText");
        set!(editor_comment, "EditorCommentText");
        set!(editor_current_query, "EditorCurrentQueryBackground");
        set!(editor_selection, "EditorSelectionBackground");
        set!(rail, "StrataRail");
        set!(accent, "StrataAccent");
        set!(text_faint, "StrataTextFaint");
        for (i, key) in ["AppChartPaletteColor1", "AppChartPaletteColor2", "AppChartPaletteColor3", "AppChartPaletteColor4", "AppChartPaletteColor5"].iter().enumerate() {
            if let Some(c) = get(key) {
                p.chart[i] = c;
            }
        }
        if !keys.contains_key("StrataRail") {
            p.rail = mix(p.sidebar, Color32::BLACK, if p.dark { 0.25 } else { 0.04 });
        }
        if !keys.contains_key("EditorSelectionBackground") && keys.contains_key("StrataAccent") {
            p.editor_selection = mix(p.content, p.accent, 0.35);
        }
        set!(accent2, "StrataAccent2");
        p.editor_bg = p.content;
        p.on_accent = if luminance(p.accent) > 0.6 { Color32::from_rgb(10, 14, 20) } else { Color32::WHITE };
        if !keys.contains_key("StrataGround") {
            p.ground = p.window;
        }
        if !keys.contains_key("StrataGlass") {
            let [r, g, b, _] = mix(p.sidebar, if p.dark { Color32::WHITE } else { Color32::WHITE }, if p.dark { 0.04 } else { 0.5 }).to_array();
            p.glass = Color32::from_rgba_unmultiplied(r, g, b, if p.dark { 200 } else { 196 });
        }
        if let Some(c) = get("StrataGround") {
            p.ground = c;
        }
        if let Some(c) = get("StrataGlass") {
            p.glass = c;
        }
        p
    }

    /// Strata's default, "Obsidian": flat warm near-black surfaces, translucent-grey hairlines,
    /// soft grey controls, one ochre accent. Lines are the opaque result of white at 8 % and 14 % over the panel tone, because
    /// several painters `mix` palette colours, which ignores alpha.
    pub fn night() -> Palette {
        Palette {
            name: "Strata Night".into(),
            dark: true,
            window: hex("#141312").unwrap(),
            content: hex("#141312").unwrap(),
            sidebar: hex("#1D1C1B").unwrap(),
            rail: hex("#1D1C1B").unwrap(),
            surface: hex("#232120").unwrap(),
            surface_hover: hex("#2C2A28").unwrap(),
            border: hex("#3A3836").unwrap(),
            separator: hex("#2A2928").unwrap(),
            text: hex("#ECE9E4").unwrap(),
            text_dim: hex("#A9A49C").unwrap(),
            text_faint: hex("#9C968C").unwrap(),
            accent: hex("#F0AB43").unwrap(),
            on_accent: hex("#1F1405").unwrap(),
            success: hex("#86D596").unwrap(),
            warning: hex("#F0AB43").unwrap(),
            danger: hex("#F47C66").unwrap(),
            tab_selected: hex("#2C2A28").unwrap(),
            grid_header: hex("#252525").unwrap(),
            grid_header_text: hex("#DEDEDE").unwrap(),
            grid_even: hex("#1E1E1E").unwrap(),
            grid_odd: hex("#2B2B2B").unwrap(),
            grid_text: hex("#DEDEDE").unwrap(),
            grid_number: hex("#DEDEDE").unwrap(),
            grid_placeholder: hex("#9C968C").unwrap(),
            grid_separator: hex("#383838").unwrap(),
            grid_selection: hex("#0058D0").unwrap(),
            grid_selection_border: hex("#FFFFFF").unwrap(),
            modified: hex("#3A2C14").unwrap(),
            inserted: hex("#201F1E").unwrap(),
            deleted: hex("#3D1D18").unwrap(),
            editor_bg: hex("#141312").unwrap(),
            editor_text: hex("#A9A49C").unwrap(),
            editor_keyword: hex("#ECE9E4").unwrap(),
            editor_function: hex("#ECE9E4").unwrap(),
            editor_string: hex("#B8B2A8").unwrap(),
            editor_dquote: hex("#B8B2A8").unwrap(),
            editor_backtick: hex("#B8B2A8").unwrap(),
            editor_number: hex("#B8B2A8").unwrap(),
            editor_comment: hex("#9C968C").unwrap(),
            editor_current_query: hex("#1B1A19").unwrap(),
            editor_selection: hex("#3A2C14").unwrap(),
            chart: [hex("#3987E5").unwrap(), hex("#D95926").unwrap(), hex("#199E70").unwrap(), hex("#C98500").unwrap(), hex("#D55181").unwrap()],
            ground: Color32::from_rgba_unmultiplied(20, 19, 18, 236),
            glass: hex("#1D1C1B").unwrap(),
            glass_stroke: hex("#2A2928").unwrap(),
            accent2: hex("#54D1B8").unwrap(),
        }
    }

    /// "Quartz", the light counterpart of Obsidian.
    pub fn day() -> Palette {
        Palette {
            name: "Strata Day".into(),
            dark: false,
            window: hex("#FCFBF9").unwrap(),
            content: hex("#FCFBF9").unwrap(),
            sidebar: hex("#F3F1ED").unwrap(),
            rail: hex("#F3F1ED").unwrap(),
            surface: hex("#FFFFFF").unwrap(),
            surface_hover: hex("#ECE9E3").unwrap(),
            border: hex("#D9D6D0").unwrap(),
            separator: hex("#E7E5E1").unwrap(),
            text: hex("#1B1916").unwrap(),
            text_dim: hex("#57524A").unwrap(),
            text_faint: hex("#625D54").unwrap(),
            accent: hex("#945600").unwrap(),
            on_accent: Color32::WHITE,
            success: hex("#1F7235").unwrap(),
            warning: hex("#945600").unwrap(),
            danger: hex("#B23520").unwrap(),
            tab_selected: hex("#ECE9E3").unwrap(),
            grid_header: hex("#EEEEEE").unwrap(),
            grid_header_text: hex("#1B1916").unwrap(),
            grid_even: hex("#FFFFFF").unwrap(),
            grid_odd: hex("#F2F2F2").unwrap(),
            grid_text: hex("#1B1916").unwrap(),
            grid_number: hex("#1B1916").unwrap(),
            grid_placeholder: hex("#625D54").unwrap(),
            grid_separator: hex("#E3E2E0").unwrap(),
            grid_selection: hex("#0058D0").unwrap(),
            grid_selection_border: hex("#FFFFFF").unwrap(),
            modified: hex("#F8E9CC").unwrap(),
            inserted: hex("#EDEBE8").unwrap(),
            deleted: hex("#F9E0DA").unwrap(),
            editor_bg: hex("#FCFBF9").unwrap(),
            editor_text: hex("#57524A").unwrap(),
            editor_keyword: hex("#1B1916").unwrap(),
            editor_function: hex("#1B1916").unwrap(),
            editor_string: hex("#555048").unwrap(),
            editor_dquote: hex("#555048").unwrap(),
            editor_backtick: hex("#555048").unwrap(),
            editor_number: hex("#555048").unwrap(),
            editor_comment: hex("#625D54").unwrap(),
            editor_current_query: hex("#F3F1ED").unwrap(),
            editor_selection: hex("#F8E9CC").unwrap(),
            chart: [hex("#2A78D6").unwrap(), hex("#EB6834").unwrap(), hex("#1BAF7A").unwrap(), hex("#EDA100").unwrap(), hex("#E87BA4").unwrap()],
            ground: Color32::from_rgba_unmultiplied(252, 251, 249, 230),
            glass: hex("#F3F1ED").unwrap(),
            glass_stroke: hex("#E7E5E1").unwrap(),
            accent2: hex("#0A6E5C").unwrap(),
        }
    }

    pub fn builtins() -> Vec<Palette> {
        let night = Palette::night();
        let day = Palette::day();
        let k = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>();
        let jade = Palette::from_keys(
            "Jade",
            &night,
            &k(&[
                ("AppWindowBackground", "#0B2127"),
                ("AppContentBackground", "#002B36"),
                ("AppSidebarBackground", "#00212B"),
                ("AppControlBackground", "#003440"),
                ("AppControlBorder", "#1E4550"),
                ("AppTabSelectedBackground", "#0F4A5B"),
                ("AppTabHoverBackground", "#005167"),
                ("GridHeaderBackground", "#002B36"),
                ("GridEvenRowBackground", "#002B36"),
                ("GridOddRowBackground", "#04313D"),
                ("GridSeparator", "#0F3E4A"),
                ("EditorKeywordText", "#529BD8"),
                ("EditorFunctionText", "#7CB342"),
                ("EditorCommentText", "#2E8B57"),
                ("EditorNumberText", "#B57AFF"),
                ("EditorSingleQuotedText", "#E0A07E"),
                ("EditorCurrentQueryBackground", "#003745"),
                ("StrataAccent", "#2BB3A3"),
            ]),
        );
        let graphite = Palette::from_keys(
            "Graphite",
            &night,
            &k(&[
                ("AppWindowBackground", "#161616"),
                ("AppContentBackground", "#1B1B1B"),
                ("AppSidebarBackground", "#141414"),
                ("AppControlBackground", "#232323"),
                ("AppControlBorder", "#323232"),
                ("AppTabSelectedBackground", "#2A2A2A"),
                ("AppTabHoverBackground", "#2C2C2C"),
                ("GridHeaderBackground", "#1F1F1F"),
                ("GridEvenRowBackground", "#1B1B1B"),
                ("GridOddRowBackground", "#202020"),
                ("GridSeparator", "#2A2A2A"),
                ("EditorCurrentQueryBackground", "#202020"),
                ("StrataAccent", "#FF8A3D"),
            ]),
        );
        let dusk = Palette::from_keys(
            "Dusk",
            &night,
            &k(&[
                ("AppWindowBackground", "#17121F"),
                ("AppContentBackground", "#1B1524"),
                ("AppSidebarBackground", "#140F1B"),
                ("AppControlBackground", "#241C31"),
                ("AppControlBorder", "#352A47"),
                ("AppTabSelectedBackground", "#2B2140"),
                ("AppTabHoverBackground", "#2E2442"),
                ("GridHeaderBackground", "#1F1829"),
                ("GridEvenRowBackground", "#1B1524"),
                ("GridOddRowBackground", "#1F182A"),
                ("GridSeparator", "#2A2138"),
                ("EditorCurrentQueryBackground", "#21192D"),
                ("StrataAccent", "#E879F9"),
            ]),
        );
        let paper = Palette::from_keys(
            "Paper",
            &day,
            &k(&[
                ("AppWindowBackground", "#F3EFE6"),
                ("AppContentBackground", "#FBF8F1"),
                ("AppSidebarBackground", "#EFEADF"),
                ("AppControlBackground", "#FFFDF8"),
                ("AppControlBorder", "#DDD5C4"),
                ("GridHeaderBackground", "#F4EFE4"),
                ("GridEvenRowBackground", "#FBF8F1"),
                ("GridOddRowBackground", "#F6F2E9"),
                ("GridSeparator", "#EAE3D5"),
                ("EditorCurrentQueryBackground", "#F4EFE4"),
                ("StrataAccent", "#B4541B"),
            ]),
        );
        vec![night, day, jade, graphite, dusk, paper]
    }

    /// Reads a theme JSON file (`{"name", "base", "colors": {...}}`).
    pub fn load_file(path: &std::path::Path) -> anyhow::Result<Palette> {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let name = v["name"].as_str().unwrap_or("Imported").to_string();
        let base = if v["base"].as_str() == Some("light") { Palette::day() } else { Palette::night() };
        let colors: BTreeMap<String, String> = v["colors"]
            .as_object()
            .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default();
        if colors.is_empty() {
            anyhow::bail!("No colors in theme file");
        }
        Ok(Palette::from_keys(&name, &base, &colors))
    }

    /// Fill of floating sheets: menus, popups, tooltips, toasts. `ui::glass` turns exactly this
    /// colour into Embun glass, so nothing else may use it. Opaque, so a sheet stays readable
    /// wherever the glass is off.
    pub fn sheet(&self) -> Color32 {
        if self.dark {
            let [r, g, b, _] = mix(self.glass_opaque(), self.surface_hover, 0.6).to_array();
            Color32::from_rgb(r, g, b)
        } else {
            // A hair off white, so the glass hook can tell a sheet from a white control.
            Color32::from_rgb(254, 254, 253)
        }
    }

    /// Soft control fill at rest: buttons, groups, fields, chips. White 7.5 % / black 5 %.
    pub fn control(&self) -> Color32 {
        if self.dark { Color32::from_white_alpha(19) } else { Color32::from_black_alpha(13) }
    }

    /// Soft control fill under the pointer: white 12 % / black 8.5 %.
    pub fn control_hover(&self) -> Color32 {
        if self.dark { Color32::from_white_alpha(31) } else { Color32::from_black_alpha(22) }
    }

    /// The chosen item inside a group or segmented control: white 14 % / solid white.
    pub fn control_on(&self) -> Color32 {
        if self.dark { Color32::from_white_alpha(36) } else { Color32::WHITE }
    }

    /// The glass tone without its transparency.
    pub fn glass_opaque(&self) -> Color32 {
        let [r, g, b, a] = self.glass.to_srgba_unmultiplied();
        let _ = a;
        Color32::from_rgb(r, g, b)
    }

    pub fn apply(&self, ctx: &egui::Context, scale: f32) {
        let mut v = if self.dark { Visuals::dark() } else { Visuals::light() };
        let r = CornerRadius::same(8);
        v.override_text_color = Some(self.text);
        // Panels paint their own glass or solid cards; the default fill would cover the ground.
        v.panel_fill = Color32::TRANSPARENT;
        v.window_fill = self.sheet();
        v.extreme_bg_color = if self.dark { mix(self.content, Color32::BLACK, 0.25) } else { mix(self.content, self.window, 0.4) };
        v.faint_bg_color = self.grid_odd;
        v.code_bg_color = self.surface_hover;
        v.window_stroke = Stroke::new(1.0, self.glass_stroke);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(12);
        v.hyperlink_color = self.accent2;
        // Neutral selection: chosen controls read by weight and fill, not by a second hue.
        v.selection.bg_fill = mix(self.content, self.text, if self.dark { 0.16 } else { 0.12 });
        v.selection.stroke = Stroke::new(1.0, self.text);
        v.warn_fg_color = self.warning;
        v.error_fg_color = self.danger;
        v.text_cursor.stroke = Stroke::new(1.5, self.text);
        v.striped = false;
        v.widgets.noninteractive.bg_fill = self.surface;
        v.widgets.noninteractive.weak_bg_fill = self.surface;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.separator);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.noninteractive.corner_radius = r;
        // Soft controls: a fill and nothing else. No borders, no accent on press.
        v.widgets.inactive.bg_fill = self.control();
        v.widgets.inactive.weak_bg_fill = self.control();
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.inactive.corner_radius = r;
        v.widgets.hovered.bg_fill = self.control_hover();
        v.widgets.hovered.weak_bg_fill = self.control_hover();
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.hovered.corner_radius = r;
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.bg_fill = self.control();
        v.widgets.active.weak_bg_fill = self.control();
        v.widgets.active.bg_stroke = Stroke::NONE;
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.active.corner_radius = r;
        v.widgets.active.expansion = 0.0;
        v.widgets.open = v.widgets.hovered;
        v.popup_shadow = egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(if self.dark { 140 } else { 46 }) };
        v.window_shadow = egui::Shadow { offset: [0, 16], blur: 48, spread: 0, color: Color32::from_black_alpha(if self.dark { 150 } else { 56 }) };
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_visuals(v);
        ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.menu_margin = egui::Margin::same(8);
            s.spacing.window_margin = egui::Margin::same(16);
            s.spacing.button_padding = egui::vec2(12.0, 6.0);
            s.spacing.interact_size.y = 30.0;
            s.spacing.scroll.bar_width = 8.0;
            s.spacing.scroll.floating = true;
            // Lists stop at their ends. egui's rubber band stretched past the last row and
            // sprang back, and a nested grid bounced twice (once per scroll area).
            s.spacing.scroll.kinetic.rubber_band = false;
            s.interaction.selectable_labels = false;
            // Unfolds and fades run on the design's "settle" duration.
            s.animation_time = crate::ui::motion::SETTLE as f32;
        });
        ctx.set_zoom_factor(scale);
    }
}
