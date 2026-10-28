//! Shared widgets: vector icons, buttons, pills, tiles, toasts, fonts.

pub mod about;
pub mod assistant;
pub mod diagram;
pub mod dialogs;
pub mod editor;
pub mod geo;
pub mod grid;
pub mod home;
pub mod json_tree;
pub mod location_picker;
pub mod metrics;
pub mod object_editor;
pub mod query_tab;
pub mod settings;
pub mod table_tab;
pub mod viewer;
pub mod workspace;

use crate::model::DriverKind;
use crate::theme::{mix, Palette};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, Ui};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

/// Ground opacity in 1/1000ths, set from the translucency setting each frame.
static GROUND_OPACITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

pub fn set_ground_opacity(o: f32) {
    GROUND_OPACITY.store((o.clamp(0.0, 1.0) * 1000.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

static PALETTE: LazyLock<RwLock<Arc<Palette>>> = LazyLock::new(|| RwLock::new(Arc::new(Palette::night())));

pub fn p() -> Arc<Palette> {
    PALETTE.read().map(|g| g.clone()).unwrap_or_else(|_| Arc::new(Palette::night()))
}

pub fn set_palette(pal: Palette) {
    if let Ok(mut g) = PALETTE.write() {
        *g = Arc::new(pal);
    }
}

/// The full icon set; not every glyph is used on every screen.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    Plus,
    Gear,
    Database,
    Table,
    View,
    Function,
    Play,
    PlayAll,
    Stop,
    Refresh,
    Filter,
    Trash,
    Check,
    Close,
    Lock,
    Unlock,
    Code,
    Chart,
    Diagram,
    Export,
    Import,
    Terminal,
    Users,
    Activity,
    Home,
    Sparkle,
    Key,
    Link,
    Columns,
    Rows,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Copy,
    Edit,
    Folder,
    Plug,
    Eye,
    Clock,
    Star,
    Sidebar,
    Backup,
    Layers,
    Shield,
    Graph,
    Pin,
    More,
    Bolt,
    Puzzle,
    Fingerprint,
    Globe,
    Tree,
    Window,
    Bell,
    Wand,
}

/// Draws a stroke icon inside `rect`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0;
    let st = Stroke::new((1.5 * u).max(1.2), color);
    let p = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], st);
    let poly = |pts: Vec<Pos2>| painter.add(egui::Shape::line(pts, st));
    let closed = |pts: Vec<Pos2>| painter.add(egui::Shape::closed_line(pts, st));
    let rrect = |min: Pos2, max: Pos2, r: f32| painter.rect_stroke(Rect::from_min_max(min, max), CornerRadius::same(r as u8), st, egui::StrokeKind::Middle);
    match icon {
        Icon::Search => {
            painter.circle_stroke(p(7.0, 7.0), 4.5 * u, st);
            line(p(10.4, 10.4), p(14.0, 14.0));
        }
        Icon::Plus => {
            line(p(8.0, 3.0), p(8.0, 13.0));
            line(p(3.0, 8.0), p(13.0, 8.0));
        }
        Icon::Gear => {
            painter.circle_stroke(c, 2.2 * u, st);
            painter.circle_stroke(c, 5.0 * u, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(c + d * 5.0 * u, c + d * 6.8 * u);
            }
        }
        Icon::Database => {
            let (l, r) = (3.5, 12.5);
            painter.add(egui::Shape::ellipse_stroke(p(8.0, 4.0), vec2(4.5 * u, 1.8 * u), st));
            line(p(l, 4.0), p(l, 12.0));
            line(p(r, 4.0), p(r, 12.0));
            poly((0..=12).map(|i| {
                let t = std::f32::consts::PI * i as f32 / 12.0;
                p(8.0 - 4.5 * t.cos(), 8.0 + 1.8 * t.sin())
            }).collect());
            poly((0..=12).map(|i| {
                let t = std::f32::consts::PI * i as f32 / 12.0;
                p(8.0 - 4.5 * t.cos(), 12.0 + 1.8 * t.sin())
            }).collect());
        }
        Icon::Table | Icon::Rows => {
            rrect(p(2.5, 3.0), p(13.5, 13.0), 2.0 * u);
            line(p(2.5, 6.5), p(13.5, 6.5));
            line(p(2.5, 9.8), p(13.5, 9.8));
            if icon == Icon::Table {
                line(p(6.5, 6.5), p(6.5, 13.0));
            }
        }
        Icon::Columns => {
            rrect(p(2.5, 3.0), p(13.5, 13.0), 2.0 * u);
            line(p(6.2, 3.0), p(6.2, 13.0));
            line(p(9.8, 3.0), p(9.8, 13.0));
        }
        Icon::View => {
            rrect(p(2.5, 3.0), p(13.5, 13.0), 2.0 * u);
            line(p(2.5, 6.5), p(13.5, 6.5));
            painter.circle_stroke(p(8.0, 9.8), 1.6 * u, st);
        }
        Icon::Function => {
            poly(vec![p(11.0, 3.0), p(9.5, 3.0), p(8.0, 4.5), p(7.0, 12.0), p(5.5, 13.5), p(4.0, 13.5)]);
            line(p(5.0, 7.0), p(10.5, 7.0));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(vec![p(5.0, 3.5), p(13.0, 8.0), p(5.0, 12.5)], color, Stroke::NONE));
        }
        Icon::PlayAll => {
            painter.add(egui::Shape::convex_polygon(vec![p(3.0, 3.5), p(9.5, 8.0), p(3.0, 12.5)], color, Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![p(8.0, 3.5), p(14.5, 8.0), p(8.0, 12.5)], color, Stroke::NONE));
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_min_max(p(4.0, 4.0), p(12.0, 12.0)), CornerRadius::same((1.5 * u) as u8), color);
        }
        Icon::Refresh => {
            poly((0..=20).map(|i| {
                let a = -0.6 + i as f32 * 5.0 / 20.0;
                p(8.0 + 5.0 * a.cos(), 8.0 + 5.0 * a.sin())
            }).collect());
            let end = p(8.0 + 5.0 * (-0.6f32).cos(), 8.0 + 5.0 * (-0.6f32).sin());
            line(end, end + vec2(0.5, -3.0) * u);
            line(end, end + vec2(-2.8, -0.6) * u);
        }
        Icon::Filter => {
            closed(vec![p(2.5, 3.5), p(13.5, 3.5), p(9.3, 8.5), p(9.3, 13.0), p(6.7, 11.8), p(6.7, 8.5)]);
        }
        Icon::Trash => {
            line(p(2.5, 4.5), p(13.5, 4.5));
            poly(vec![p(6.0, 4.5), p(6.5, 2.5), p(9.5, 2.5), p(10.0, 4.5)]);
            poly(vec![p(4.0, 4.5), p(4.8, 13.5), p(11.2, 13.5), p(12.0, 4.5)]);
            line(p(6.8, 7.0), p(6.8, 11.0));
            line(p(9.2, 7.0), p(9.2, 11.0));
        }
        Icon::Check => {
            poly(vec![p(3.0, 8.5), p(6.5, 12.0), p(13.0, 4.5)]);
        }
        Icon::Close => {
            line(p(4.0, 4.0), p(12.0, 12.0));
            line(p(12.0, 4.0), p(4.0, 12.0));
        }
        Icon::Lock | Icon::Unlock => {
            rrect(p(3.5, 7.0), p(12.5, 13.5), 1.5 * u);
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 * std::f32::consts::PI / 12.0;
                    p(8.0 + 2.8 * a.cos(), 5.5 + 2.8 * a.sin())
                })
                .collect();
            let mut arc = vec![p(5.2, 7.0)];
            arc.extend(pts);
            if icon == Icon::Lock {
                arc.push(p(10.8, 7.0));
            } else {
                arc.push(p(10.8, 5.0));
            }
            poly(arc);
            painter.circle_filled(p(8.0, 10.2), 1.0 * u, color);
        }
        Icon::Code => {
            poly(vec![p(5.5, 4.0), p(2.0, 8.0), p(5.5, 12.0)]);
            poly(vec![p(10.5, 4.0), p(14.0, 8.0), p(10.5, 12.0)]);
            line(p(9.0, 3.0), p(7.0, 13.0));
        }
        Icon::Chart => {
            line(p(2.5, 13.5), p(13.5, 13.5));
            painter.rect_filled(Rect::from_min_max(p(3.5, 8.0), p(5.8, 13.0)), CornerRadius::same(1), color);
            painter.rect_filled(Rect::from_min_max(p(6.9, 4.0), p(9.2, 13.0)), CornerRadius::same(1), color);
            painter.rect_filled(Rect::from_min_max(p(10.3, 6.5), p(12.6, 13.0)), CornerRadius::same(1), color);
        }
        Icon::Diagram => {
            rrect(p(2.0, 2.5), p(7.0, 6.5), 1.0 * u);
            rrect(p(9.0, 9.5), p(14.0, 13.5), 1.0 * u);
            poly(vec![p(4.5, 6.5), p(4.5, 11.5), p(9.0, 11.5)]);
        }
        Icon::Export | Icon::Backup => {
            poly(vec![p(2.5, 10.0), p(2.5, 13.5), p(13.5, 13.5), p(13.5, 10.0)]);
            line(p(8.0, 2.5), p(8.0, 10.0));
            poly(vec![p(5.0, 5.5), p(8.0, 2.5), p(11.0, 5.5)]);
        }
        Icon::Import => {
            poly(vec![p(2.5, 10.0), p(2.5, 13.5), p(13.5, 13.5), p(13.5, 10.0)]);
            line(p(8.0, 2.5), p(8.0, 10.0));
            poly(vec![p(5.0, 7.0), p(8.0, 10.0), p(11.0, 7.0)]);
        }
        Icon::Terminal => {
            rrect(p(2.0, 3.0), p(14.0, 13.0), 2.0 * u);
            poly(vec![p(4.5, 6.0), p(7.0, 8.0), p(4.5, 10.0)]);
            line(p(8.5, 10.5), p(11.5, 10.5));
        }
        Icon::Users => {
            painter.circle_stroke(p(6.0, 5.5), 2.3 * u, st);
            poly((0..=12).map(|i| {
                let a = std::f32::consts::PI + i as f32 * std::f32::consts::PI / 12.0;
                p(6.0 + 4.0 * a.cos(), 13.5 + 4.0 * a.sin())
            }).collect());
            painter.circle_stroke(p(11.5, 6.0), 1.8 * u, st);
            poly(vec![p(10.8, 9.5), p(12.5, 9.5), p(14.5, 12.5)]);
        }
        Icon::Activity => {
            poly(vec![p(1.5, 8.5), p(4.5, 8.5), p(6.5, 3.5), p(9.5, 12.5), p(11.5, 8.5), p(14.5, 8.5)]);
        }
        Icon::Home => {
            poly(vec![p(2.0, 8.0), p(8.0, 2.5), p(14.0, 8.0)]);
            poly(vec![p(4.0, 6.5), p(4.0, 13.5), p(12.0, 13.5), p(12.0, 6.5)]);
        }
        Icon::Sparkle => {
            let star = |cx: f32, cy: f32, r: f32| {
                vec![p(cx, cy - r), p(cx + r * 0.28, cy - r * 0.28), p(cx + r, cy), p(cx + r * 0.28, cy + r * 0.28), p(cx, cy + r), p(cx - r * 0.28, cy + r * 0.28), p(cx - r, cy), p(cx - r * 0.28, cy - r * 0.28)]
            };
            painter.add(egui::Shape::convex_polygon(star(7.0, 8.5, 5.5), color, Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(star(12.5, 3.5, 2.2), color, Stroke::NONE));
        }
        Icon::Key => {
            painter.circle_stroke(p(5.0, 8.0), 2.8 * u, st);
            line(p(7.8, 8.0), p(14.0, 8.0));
            line(p(12.0, 8.0), p(12.0, 10.5));
            line(p(14.0, 8.0), p(14.0, 10.0));
        }
        Icon::Link => {
            line(p(3.0, 8.0), p(12.5, 8.0));
            poly(vec![p(9.5, 5.0), p(12.8, 8.0), p(9.5, 11.0)]);
        }
        Icon::ChevronLeft => {
            poly(vec![p(10.0, 3.5), p(5.5, 8.0), p(10.0, 12.5)]);
        }
        Icon::ChevronRight => {
            poly(vec![p(6.0, 3.5), p(10.5, 8.0), p(6.0, 12.5)]);
        }
        Icon::ChevronDown => {
            poly(vec![p(3.5, 6.0), p(8.0, 10.5), p(12.5, 6.0)]);
        }
        Icon::Copy => {
            rrect(p(5.0, 5.0), p(13.5, 13.5), 1.5 * u);
            poly(vec![p(10.5, 5.0), p(10.5, 2.5), p(2.5, 2.5), p(2.5, 10.5), p(5.0, 10.5)]);
        }
        Icon::Edit => {
            closed(vec![p(3.0, 13.0), p(3.5, 10.0), p(11.0, 2.5), p(13.5, 5.0), p(6.0, 12.5)]);
        }
        Icon::Folder => {
            closed(vec![p(2.0, 4.0), p(6.0, 4.0), p(7.5, 5.5), p(14.0, 5.5), p(14.0, 13.0), p(2.0, 13.0)]);
        }
        Icon::Plug => {
            line(p(6.0, 2.5), p(6.0, 5.5));
            line(p(10.0, 2.5), p(10.0, 5.5));
            poly(vec![p(3.5, 5.5), p(12.5, 5.5), p(12.5, 8.0), p(8.0, 11.5), p(3.5, 8.0), p(3.5, 5.5)]);
            line(p(8.0, 11.5), p(8.0, 14.0));
        }
        Icon::Eye => {
            poly((0..=16).map(|i| {
                let t = std::f32::consts::PI * i as f32 / 16.0;
                p(8.0 - 6.0 * t.cos(), 8.0 - 3.6 * t.sin())
            }).collect());
            poly((0..=16).map(|i| {
                let t = std::f32::consts::PI * i as f32 / 16.0;
                p(8.0 - 6.0 * t.cos(), 8.0 + 3.6 * t.sin())
            }).collect());
            painter.circle_stroke(c, 1.8 * u, st);
        }
        Icon::Clock => {
            painter.circle_stroke(c, 5.8 * u, st);
            poly(vec![p(8.0, 4.5), p(8.0, 8.0), p(10.5, 9.5)]);
        }
        Icon::Star => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let r = if i % 2 == 0 { 6.0 } else { 2.6 };
                    p(8.0 + r * a.cos(), 8.4 + r * a.sin())
                })
                .collect();
            closed(pts);
        }
        Icon::Sidebar => {
            rrect(p(2.0, 3.0), p(14.0, 13.0), 2.0 * u);
            line(p(6.0, 3.0), p(6.0, 13.0));
        }
        Icon::Layers => {
            closed(vec![p(8.0, 2.5), p(14.0, 5.5), p(8.0, 8.5), p(2.0, 5.5)]);
            poly(vec![p(2.0, 8.5), p(8.0, 11.5), p(14.0, 8.5)]);
            poly(vec![p(2.0, 11.0), p(8.0, 14.0), p(14.0, 11.0)]);
        }
        Icon::Shield => {
            closed(vec![p(8.0, 1.8), p(13.5, 4.0), p(13.2, 8.5), p(8.0, 14.2), p(2.8, 8.5), p(2.5, 4.0)]);
            poly(vec![p(5.5, 8.0), p(7.3, 9.8), p(10.6, 6.2)]);
        }
        Icon::Graph => {
            let nodes = [p(4.0, 4.0), p(12.0, 5.0), p(8.0, 12.0), p(3.0, 12.5)];
            for (a, b) in [(0, 1), (1, 2), (0, 2), (2, 3)] {
                line(nodes[a], nodes[b]);
            }
            for n in nodes {
                painter.circle_filled(n, 1.9 * u, color);
            }
        }
        Icon::Pin => {
            closed(vec![p(6.0, 2.5), p(10.0, 2.5), p(9.5, 6.5), p(12.0, 9.0), p(4.0, 9.0), p(6.5, 6.5)]);
            line(p(8.0, 9.0), p(8.0, 14.0));
        }
        Icon::More => {
            for x in [4.0, 8.0, 12.0] {
                painter.circle_filled(p(x, 8.0), 1.3 * u, color);
            }
        }
        Icon::Bolt => {
            painter.add(egui::Shape::convex_polygon(vec![p(9.5, 1.5), p(3.5, 9.0), p(7.5, 9.0)], color, Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![p(8.5, 7.0), p(12.5, 7.0), p(6.5, 14.5)], color, Stroke::NONE));
        }
        Icon::Puzzle => {
            closed(vec![p(3.0, 5.0), p(6.0, 5.0), p(6.0, 3.5), p(9.0, 3.5), p(9.0, 5.0), p(13.0, 5.0), p(13.0, 8.0), p(11.5, 8.0), p(11.5, 11.0), p(13.0, 11.0), p(13.0, 13.5), p(3.0, 13.5)]);
        }
        Icon::Fingerprint => {
            for (i, r) in [2.0f32, 4.0, 6.0].iter().enumerate() {
                let start = std::f32::consts::PI * (1.05 + i as f32 * 0.05);
                poly((0..=12).map(|k| {
                    let a = start + k as f32 * (std::f32::consts::PI * 0.9) / 12.0;
                    p(8.0 + r * a.cos(), 9.0 + r * a.sin())
                }).collect());
            }
            line(p(8.0, 9.0), p(8.0, 13.5));
        }
        Icon::Globe => {
            painter.circle_stroke(c, 5.8 * u, st);
            painter.add(egui::Shape::ellipse_stroke(c, vec2(2.6 * u, 5.8 * u), st));
            line(p(2.2, 8.0), p(13.8, 8.0));
        }
        Icon::Tree => {
            rrect(p(2.0, 2.5), p(6.0, 5.5), 1.0 * u);
            rrect(p(9.0, 6.5), p(14.0, 9.5), 1.0 * u);
            rrect(p(9.0, 11.0), p(14.0, 14.0), 1.0 * u);
            poly(vec![p(4.0, 5.5), p(4.0, 12.5), p(9.0, 12.5)]);
            line(p(4.0, 8.0), p(9.0, 8.0));
        }
        Icon::Window => {
            rrect(p(2.0, 3.0), p(14.0, 13.0), 2.0 * u);
            line(p(2.0, 6.0), p(14.0, 6.0));
            painter.circle_filled(p(4.2, 4.5), 0.6 * u, color);
        }
        Icon::Bell => {
            poly(vec![p(3.5, 11.5), p(4.5, 10.0), p(4.5, 6.5), p(6.0, 3.8), p(8.0, 3.2), p(10.0, 3.8), p(11.5, 6.5), p(11.5, 10.0), p(12.5, 11.5), p(3.5, 11.5)]);
            line(p(7.0, 13.5), p(9.0, 13.5));
        }
        Icon::Wand => {
            line(p(3.0, 13.0), p(10.5, 5.5));
            for (a, b) in [(p(11.5, 1.8), p(11.5, 4.2)), (p(10.3, 3.0), p(12.7, 3.0)), (p(13.8, 6.5), p(13.8, 8.1)), (p(13.0, 7.3), p(14.6, 7.3))] {
                line(a, b);
            }
        }
    }
}

/// Frameless icon button with a hover plate, like a toolbar item.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    icon_button_colored(ui, icon, tip, None, false)
}

pub fn icon_button_colored(ui: &mut Ui, icon: Icon, tip: &str, tint: Option<Color32>, active: bool) -> Response {
    let pal = p();
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tip));
    let hovered = resp.hovered();
    if active || hovered {
        let fill = if active { mix(pal.surface_hover, pal.accent, 0.20) } else { pal.surface_hover };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    }
    let color = tint.unwrap_or(if active { pal.accent } else if hovered { pal.text } else { pal.text_dim });
    paint_icon(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, color);
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let pal = p();
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(pal.on_accent).strong())
            .fill(pal.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(10))
            .min_size(vec2(0.0, 32.0)),
    )
}

pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    let pal = p();
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong()).fill(pal.danger).stroke(Stroke::NONE).corner_radius(CornerRadius::same(10)).min_size(vec2(0.0, 32.0)))
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    let pal = p();
    ui.add(egui::Button::new(egui::RichText::new(text).color(pal.text)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, pal.border)).corner_radius(CornerRadius::same(10)).min_size(vec2(0.0, 32.0)))
}

pub fn pill(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let pal = p();
    let font = FontId::proportional(10.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = vec2(galley.size().x + 14.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let bg = if pal.dark { color.gamma_multiply(0.18) } else { color.gamma_multiply(0.14) };
    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Rounded brand-colored square with a soft top highlight, like Termius' host icons.
fn paint_badge_ground(painter: &egui::Painter, rect: Rect, color: Color32) {
    let size = rect.width();
    let top = mix(color, Color32::WHITE, 0.12);
    painter.rect_filled(rect, CornerRadius::same((size * 0.28) as u8), color);
    painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.center().y)), CornerRadius { nw: (size * 0.28) as u8, ne: (size * 0.28) as u8, sw: 0, se: 0 }, top.gamma_multiply(0.35));
}

/// White on dark brand colors, near-black on light ones (ClickHouse, DuckDB yellow).
fn mark_color(ground: Color32) -> Color32 {
    if crate::theme::luminance(ground) > 0.6 { Color32::from_rgb(28, 28, 30) } else { Color32::WHITE }
}

/// Simple Icons marks (CC0, see assets/db/README.md). Drivers without one keep the two-letter badge.
fn driver_logo(d: DriverKind) -> Option<&'static [u8]> {
    Some(match d {
        DriverKind::Postgres => include_bytes!("../../assets/db/postgresql.svg"),
        DriverKind::CockroachDb => include_bytes!("../../assets/db/cockroachlabs.svg"),
        DriverKind::Redshift => include_bytes!("../../assets/db/amazonredshift.svg"),
        DriverKind::MySql => include_bytes!("../../assets/db/mysql.svg"),
        DriverKind::MariaDb => include_bytes!("../../assets/db/mariadb.svg"),
        DriverKind::Sqlite => include_bytes!("../../assets/db/sqlite.svg"),
        DriverKind::LibSql => include_bytes!("../../assets/db/turso.svg"),
        DriverKind::CloudflareD1 => include_bytes!("../../assets/db/cloudflare.svg"),
        DriverKind::SqlServer => include_bytes!("../../assets/db/microsoftsqlserver.svg"),
        DriverKind::ClickHouse => include_bytes!("../../assets/db/clickhouse.svg"),
        DriverKind::Redis => include_bytes!("../../assets/db/redis.svg"),
        DriverKind::MongoDb => include_bytes!("../../assets/db/mongodb.svg"),
        DriverKind::Elasticsearch => include_bytes!("../../assets/db/elasticsearch.svg"),
        DriverKind::Oracle => include_bytes!("../../assets/db/oracle.svg"),
        DriverKind::Snowflake => include_bytes!("../../assets/db/snowflake.svg"),
        DriverKind::BigQuery => include_bytes!("../../assets/db/googlebigquery.svg"),
        DriverKind::Cassandra => include_bytes!("../../assets/db/apachecassandra.svg"),
        DriverKind::DynamoDb => include_bytes!("../../assets/db/amazondynamodb.svg"),
        DriverKind::DuckDb => include_bytes!("../../assets/db/duckdb.svg"),
        DriverKind::Greenplum | DriverKind::Vertica => return None,
    })
}

fn driver_logo_uri(d: DriverKind) -> String {
    format!("bytes://strata/db/{d:?}.svg")
}

/// Registers the SVG loader and every driver mark; call once at startup.
pub fn install_driver_logos(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    for d in DriverKind::ALL {
        if let Some(bytes) = driver_logo(d) {
            ctx.include_bytes(driver_logo_uri(d), bytes);
        }
    }
}

/// Paints just the driver's logo, centered in `rect` at `scale` of its width. False when the driver has
/// no mark or the SVG is still being rasterized.
pub fn paint_driver_mark(painter: &egui::Painter, rect: Rect, d: DriverKind, scale: f32, tint: Color32) -> bool {
    if driver_logo(d).is_none() {
        return false;
    }
    let side = rect.width().min(rect.height()) * scale;
    let px = (side * painter.ctx().pixels_per_point()).round() as u32;
    let hint = egui::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: true };
    let Ok(egui::load::TexturePoll::Ready { texture }) = painter.ctx().try_load_texture(&driver_logo_uri(d), egui::TextureOptions::LINEAR, hint) else {
        return false;
    };
    let logo = Rect::from_center_size(rect.center(), texture.size * (side / texture.size.max_elem()));
    painter.image(texture.id, logo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
    true
}

/// The driver's brand tile with its logo; the two-letter badge when there is no logo.
pub fn paint_driver_badge(painter: &egui::Painter, rect: Rect, d: DriverKind) {
    let ground = crate::theme::rgb(d.brand_color());
    paint_badge_ground(painter, rect, ground);
    if !paint_driver_mark(painter, rect, d, 0.58, mark_color(ground)) {
        painter.text(rect.center(), Align2::CENTER_CENTER, d.badge(), FontId::proportional(rect.width() * 0.38), mark_color(ground));
    }
}

pub fn driver_tile(ui: &mut Ui, d: DriverKind, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_driver_badge(ui.painter(), rect, d);
    resp
}

pub fn section_label(ui: &mut Ui, text: &str) {
    let pal = p();
    ui.label(egui::RichText::new(text.to_uppercase()).size(10.5).color(pal.text_faint).strong());
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).color(p().text_dim)
}

pub fn faint(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).color(p().text_faint)
}

/// Single-line text field with its text centered vertically. egui's default theme pins field text to the top,
/// so it sits high whenever the field is taller than one line of text; use this instead of `TextEdit::singleline`.
pub fn line_edit(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).align(Align2::LEFT_CENTER)
}

/// Search box with a leading magnifier. Returns the text edit response.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let pal = p();
    let well = if pal.dark { Color32::from_black_alpha(70) } else { Color32::from_black_alpha(10) };
    let frame = egui::Frame::new().fill(well).stroke(Stroke::new(1.0, pal.glass_stroke)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(10, 6));
    frame
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(vec2(width - 22.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 20.0), Sense::hover());
                paint_icon(ui.painter(), Rect::from_center_size(r.center(), vec2(13.0, 13.0)), Icon::Search, pal.text_faint);
                ui.add(line_edit(text).hint_text(faint(hint)).frame(egui::Frame::NONE).desired_width(width - 46.0))
            })
            .inner
        })
        .inner
}

pub fn card_frame() -> egui::Frame {
    let pal = p();
    egui::Frame::new().fill(pal.content).stroke(Stroke::new(1.0, pal.glass_stroke)).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(14))
}

pub fn empty_state(ui: &mut Ui, icon: Icon, title: &str, subtitle: &str) {
    let pal = p();
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.28).max(20.0));
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(16), pal.surface);
        paint_icon(ui.painter(), Rect::from_center_size(r.center(), vec2(26.0, 26.0)), icon, pal.text_faint);
        ui.add_space(10.0);
        ui.label(egui::RichText::new(title).size(15.0).strong());
        ui.label(dim(subtitle));
    });
}

/// Small segmented control.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let pal = p();
    let mut changed = false;
    let well = if pal.dark { Color32::from_black_alpha(80) } else { Color32::from_black_alpha(12) };
    egui::Frame::new().fill(well).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (v, label) in options {
                let selected = *value == *v;
                let text = egui::RichText::new(*label).size(12.0).color(if selected { pal.text } else { pal.text_dim });
                let b = egui::Button::new(text).fill(if selected { pal.surface_hover } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(CornerRadius::same(8)).min_size(vec2(0.0, 26.0));
                if ui.add(b).clicked() && !selected {
                    *value = *v;
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Shared chrome surface. Solid fill keeps panels consistent over any desktop.
pub fn glass_frame() -> egui::Frame {
    let pal = p();
    egui::Frame::new()
        .fill(pal.sidebar)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

/// A solid card for data (grid, editor): never translucent, so text keeps its contrast.
pub fn content_frame() -> egui::Frame {
    let pal = p();
    egui::Frame::new().fill(pal.content).stroke(Stroke::new(1.0, pal.glass_stroke)).corner_radius(CornerRadius::same(12))
}

/// Stable window ground, with only a restrained hint of the desktop translucency.
pub fn paint_ground(ctx: &egui::Context) {
    let pal = p();
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let opacity = GROUND_OPACITY.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
    let [r, g, b, _] = pal.ground.to_srgba_unmultiplied();
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(r, g, b, (255.0 * (0.96 + opacity * 0.04)) as u8));

}

/// A colored chip with a leading dot, used for environments and statuses.
pub fn env_chip(ui: &mut Ui, label: &str, color: Color32, strong: bool) -> Response {
    let pal = p();
    let font = FontId::proportional(11.5);
    let text_color = if pal.dark { mix(color, Color32::WHITE, 0.45) } else { mix(color, Color32::BLACK, 0.35) };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
    let size = vec2(galley.size().x + 26.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), if pal.dark { 36 } else { 30 });
    ui.painter().rect_filled(rect, CornerRadius::same(255), bg);
    if strong {
        ui.painter().rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 110)), egui::StrokeKind::Inside);
    }
    if strong {
        paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 11.0, rect.center().y), vec2(11.0, 11.0)), Icon::Lock, text_color);
    } else {
        ui.painter().circle_filled(pos2(rect.left() + 11.0, rect.center().y), 3.5, color);
    }
    ui.painter().galley(pos2(rect.left() + 19.0, rect.center().y - galley.size().y / 2.0), galley, text_color);
    resp
}

/// Text color that stays readable on a tint of `color`.
pub fn readable_on_tint(color: Color32) -> Color32 {
    if p().dark { mix(color, Color32::WHITE, 0.45) } else { mix(color, Color32::BLACK, 0.35) }
}

/// A glass capsule with an icon and a short status, e.g. safe mode.
pub fn status_chip(ui: &mut Ui, icon: Icon, text: &str, color: Color32) -> Response {
    let pal = p();
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), pal.text_dim);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 40.0, 36.0), Sense::click());
    ui.painter().rect_filled(rect, CornerRadius::same(255), if resp.hovered() { pal.surface_hover } else { pal.glass });
    ui.painter().rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, pal.glass_stroke), egui::StrokeKind::Inside);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(14.0, 14.0)), icon, color);
    ui.painter().galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, pal.text_dim);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A glass capsule with a colored dot, e.g. latency.
pub fn status_dot_chip(ui: &mut Ui, text: &str, dot: Color32) -> Response {
    let pal = p();
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::monospace(11.5), pal.text_dim);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 34.0, 36.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(255), pal.glass);
    ui.painter().rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, pal.glass_stroke), egui::StrokeKind::Inside);
    ui.painter().circle_filled(pos2(rect.left() + 16.0, rect.center().y), 3.5, dot);
    ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0), galley, pal.text_dim);
    resp
}

/// 1234 → "1.2k", 48212 → "48.2k", 902114 → "902k", 3.1M.
pub fn short_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=99_999 => format!("{:.1}k", n as f64 / 1000.0),
        100_000..=999_999 => format!("{}k", n / 1000),
        1_000_000..=99_999_999 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{}M", n / 1_000_000),
    }
}

pub fn human_bytes(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub at: Instant,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.items.push(Toast { kind, text: text.into(), at: Instant::now() });
        if self.items.len() > 5 {
            self.items.remove(0);
        }
    }

    pub fn success(&mut self, t: impl Into<String>) {
        self.push(ToastKind::Success, t)
    }

    pub fn error(&mut self, t: impl Into<String>) {
        self.push(ToastKind::Error, t)
    }

    pub fn info(&mut self, t: impl Into<String>) {
        self.push(ToastKind::Info, t)
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let pal = p();
        self.items.retain(|t| t.at.elapsed() < Duration::from_secs(if t.kind == ToastKind::Error { 9 } else { 4 }));
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let mut dismiss = None;
        egui::Area::new(egui::Id::unique("toasts")).anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -34.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_max_width(380.0);
            for (i, t) in self.items.iter().enumerate() {
                let color = match t.kind {
                    ToastKind::Info => pal.accent,
                    ToastKind::Success => pal.success,
                    ToastKind::Error => pal.danger,
                };
                let r = egui::Frame::new()
                    .fill(pal.sheet())
                    .stroke(Stroke::new(1.0, pal.glass_stroke))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) })
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(4.0, 18.0), Sense::hover());
                            ui.painter().rect_filled(r, CornerRadius::same(2), color);
                            ui.add(egui::Label::new(egui::RichText::new(&t.text).size(12.5)).wrap());
                        });
                    })
                    .response
                    .interact(Sense::click());
                if r.clicked() {
                    dismiss = Some(i);
                }
                ui.add_space(6.0);
            }
        });
        if let Some(i) = dismiss {
            self.items.remove(i);
        }
    }
}

/// Bundled Geist (OFL) for a consistent look on every platform; system fonts stay as fallbacks
/// for scripts Geist lacks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let bundled: [(&str, &'static [u8]); 4] = [
        ("geist", include_bytes!("../../assets/fonts/Geist-Regular.ttf")),
        ("geist-medium", include_bytes!("../../assets/fonts/Geist-Medium.ttf")),
        ("geist-semibold", include_bytes!("../../assets/fonts/Geist-SemiBold.ttf")),
        ("geist-mono", include_bytes!("../../assets/fonts/GeistMono-Regular.ttf")),
    ];
    for (name, bytes) in bundled {
        fonts.font_data.insert(name.to_string(), Arc::new(egui::FontData::from_static(bytes)));
    }
    let fallbacks = [
        "/System/Library/Fonts/SFNS.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    let fallback = fallbacks.iter().find_map(|p| std::fs::read(p).ok());
    if let Some(bytes) = fallback {
        fonts.font_data.insert("system-ui".into(), Arc::new(egui::FontData::from_owned(bytes)));
    }
    let prop = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    prop.insert(0, "geist".into());
    if fonts.font_data.contains_key("system-ui") {
        prop.insert(1, "system-ui".into());
    }
    let defaults = prop.clone();
    let mono = fonts.families.entry(egui::FontFamily::Monospace).or_default();
    mono.insert(0, "geist-mono".into());
    if fonts.font_data.contains_key("system-ui") {
        mono.push("system-ui".into());
    }
    for (family, first) in [("medium", "geist-medium"), ("semibold", "geist-semibold")] {
        let mut list = vec![first.to_string()];
        list.extend(defaults.iter().cloned());
        fonts.families.insert(egui::FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

/// Geist SemiBold at `size`, for titles and emphasis (egui has no synthetic bold).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("medium".into()))
}

/// A heading in SemiBold.
pub fn title(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text.into()).font(semibold(size)).color(p().text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_driver_logo_rasterizes_as_a_white_mark() {
        let ctx = egui::Context::default();
        install_driver_logos(&ctx);
        let hint = egui::SizeHint::Size { width: 48, height: 48, maintain_aspect_ratio: true };
        let mut missing = Vec::new();
        for d in DriverKind::ALL {
            if driver_logo(d).is_none() {
                missing.push(d);
                continue;
            }
            let Ok(egui::load::ImagePoll::Ready { image }) = ctx.try_load_image(&driver_logo_uri(d), hint) else { panic!("{d:?} logo did not load") };
            // Pixels are premultiplied, so only fully opaque ones show the true fill.
            let inked: Vec<_> = image.pixels.iter().filter(|p| p.a() == 255).collect();
            assert!(inked.len() > 40, "{d:?} logo is nearly empty");
            assert!(inked.iter().all(|p| p.r() > 200 && p.g() > 200 && p.b() > 200), "{d:?} logo is not white, so tinting would darken it");
        }
        assert_eq!(missing, [DriverKind::Greenplum, DriverKind::Vertica]);
    }

    #[test]
    fn short_counts_read_like_the_explorer() {
        assert_eq!(short_count(64), "64");
        assert_eq!(short_count(48_212), "48.2k");
        assert_eq!(short_count(902_114), "902k");
        assert_eq!(short_count(3_100_000), "3.1M");
        assert_eq!(human_bytes(2_516_582), "2.4 MB");
    }
}

/// Copies text to the clipboard through egui (works on every backend).
pub fn copy(ctx: &egui::Context, text: impl Into<String>) {
    ctx.copy_text(text.into());
}

