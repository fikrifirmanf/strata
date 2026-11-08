//! Embun glass for floating sheets only: menus, popups, tooltips, toasts and the changes dock.
//!
//! Everything else in Strata is flat. An output hook finds the rounded rectangles filled with
//! the palette's sheet colour (`Palette::sheet`, which nothing else uses) and replaces their fill
//! with Embun's frosted glass, rendered from the app content beneath them. Text and icons on the
//! sheet are drawn by egui as usual. Each sheet has its own atlas slot, so sheets never merge.
//! Without a wgpu renderer, or with Reduce Transparency on, sheets keep their opaque fill.
use eframe::egui_wgpu::{wgpu, RenderState};
use egui::{pos2, Color32, CornerRadius, Rect, Shape, TextureId};
use embun::{
    render_glass::{Backdrop, Frame, GlassRenderer, GlassShape, MAX_SHAPES},
    Appearance, Theme,
};
use std::sync::Mutex;
#[path = "glass_scene.rs"]
mod scene;

// Egui samples native textures as gamma-encoded colors. Using an sRGB
// atlas would decode twice and turn pale glass muddy and amber nearly black.
const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIDE: u32 = 2048;
const PAD: f32 = 8.0;

#[derive(Clone, Copy)]
struct Material {
    level: f32,
    reduced: bool,
}

/// Glass slider position: toward Embun's "clear" end, so the content beneath shows through.
const SHEET_LEVEL: f32 = 0.2;
/// How strongly a sheet is tinted with its own palette colour: enough to hold text contrast,
/// little enough that the grid and editor stay visible behind a menu.
const SHEET_TINT: f32 = 0.22;
/// Frost blur in points. macOS uses 16; a lighter frost keeps shapes beneath recognisable.
const SHEET_BLUR: f32 = 9.0;

pub fn configure(ctx: &egui::Context, reduced: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("glass-material"), Material { level: SHEET_LEVEL, reduced }));
}

pub fn install(cc: &eframe::CreationContext<'_>) {
    let Some(rs) = cc.wgpu_render_state.clone() else {
        return;
    };
    cc.egui_ctx
        .add_plugin(GlassPlugin(Mutex::new(GlassAtlas::new(rs))));
}

struct GlassPlugin(Mutex<GlassAtlas>);
impl egui::Plugin for GlassPlugin {
    fn debug_name(&self) -> &'static str {
        "Embun liquid glass"
    }
    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        if let Ok(mut atlas) = self.0.lock() {
            atlas.paint(ctx, output);
        }
    }
}

struct Page {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    id: TextureId,
    // Embun uniforms belong to a backdrop. Each page needs its own buffer;
    // sharing it would make every queued pass use the final page's shapes.
    backdrop: Backdrop,
    shapes: Vec<GlassShape>,
    previous: Vec<GlassShape>,
    sources: Vec<(Rect, Rect)>,
    previous_sources: Vec<(Rect, Rect)>,
    previous_material: Option<(embun::render_glass::GlassParams, bool)>,
    x: f32,
    y: f32,
    row_h: f32,
}

impl Page {
    fn place(&mut self, size: egui::Vec2) -> Option<Rect> {
        if self.shapes.len() >= MAX_SHAPES {
            return None;
        }
        if self.x + size.x + PAD > SIDE as f32 {
            self.x = PAD;
            self.y += self.row_h + PAD;
            self.row_h = 0.0;
        }
        if self.y + size.y + PAD > SIDE as f32 {
            return None;
        }
        let r = Rect::from_min_size(pos2(self.x, self.y), size);
        self.x += size.x + PAD;
        self.row_h = self.row_h.max(size.y);
        Some(r)
    }
    fn reset(&mut self) {
        self.shapes.clear();
        self.sources.clear();
        self.x = PAD;
        self.y = PAD;
        self.row_h = 0.0;
    }
}

struct GlassAtlas {
    rs: RenderState,
    renderer: GlassRenderer,
    scene: scene::Scene,
    pages: Vec<Page>,
    dark: Option<bool>,
}

impl GlassAtlas {
    fn new(rs: RenderState) -> Self {
        let renderer = GlassRenderer::new(&rs.device, ATLAS_FORMAT);
        let scene = scene::Scene::new(&rs.device);
        Self {
            rs,
            renderer,
            scene,
            pages: Vec::new(),
            dark: None,
        }
    }

    fn add_page(&mut self, dark: bool) {
        let texture = self.rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Strata Embun material atlas"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = self.rs.renderer.write().register_native_texture(
            &self.rs.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        let backdrop = self.renderer.create_backdrop(
            &self.rs.device,
            &self.rs.queue,
            &vec![if dark { 16 } else { 230 }; (SIDE * SIDE * 4) as usize],
            SIDE,
            SIDE,
        );
        self.pages.push(Page {
            _texture: texture,
            view,
            id,
            backdrop,
            shapes: Vec::new(),
            previous: Vec::new(),
            sources: Vec::new(),
            previous_sources: Vec::new(),
            previous_material: None,
            x: PAD,
            y: PAD,
            row_h: 0.0,
        });
    }

    fn clear_pages(&mut self) {
        let mut renderer = self.rs.renderer.write();
        for page in self.pages.drain(..) {
            renderer.free_texture(&page.id);
        }
    }

    fn paint(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        self.scene.update_textures(&self.rs, &output.textures_delta);
        let pal = super::p();
        let material = ctx
            .data(|d| d.get_temp::<Material>(egui::Id::unique("glass-material")))
            .unwrap_or(Material {
                level: 0.5,
                reduced: false,
            });
        if self.dark != Some(pal.dark) {
            self.clear_pages();
            self.dark = Some(pal.dark);
        }
        for page in &mut self.pages {
            page.reset();
        }
        // Reduced transparency uses the same opaque palette and control geometry.
        if material.reduced {
            self.scene.finish(&output.textures_delta);
            return;
        }
        let limit = self.rs.device.limits().max_texture_dimension_2d as f32;
        if ctx.content_rect().size().max_elem() > limit {
            self.scene.finish(&output.textures_delta);
            return;
        }
        let mut encoder = self
            .rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Strata live Embun glass"),
            });
        let scene_changed = self.scene.render(&self.rs, ctx, output, &mut encoder, &pal);
        if self.scene.size == [0, 0] {
            self.scene.finish(&output.textures_delta);
            return;
        }
        let mut page_index = 0;
        let mut plate_count = 0;
        let viewport = ctx.content_rect();
        for entry in &mut output.shapes {
            self.adapt_shape(
                &mut entry.shape,
                entry.clip_rect.intersect(viewport),
                &pal,
                &mut page_index,
                &mut plate_count,
            );
        }
        let theme = Theme {
            appearance: if pal.dark {
                Appearance::Dark
            } else {
                Appearance::Light
            },
            glass_level: material.level,
            focused: ctx.input(|i| i.viewport().focused.unwrap_or(true)),
            reduce_transparency: material.reduced,
        };
        let mut params = material_params(theme);
        // Separate atlas plates deliberately do not merge, while each plate
        // retains Embun's measured frost, edge refraction and fringe.
        params.merge_k = 0.0;
        params.blur_sigma = params.blur_sigma.min(SHEET_BLUR);
        let mut changed = scene_changed;
        for page in self.pages.iter_mut().take(page_index + 1) {
            if page.shapes.is_empty() {
                continue;
            }
            let key = (params, pal.dark);
            if !scene_changed
                && page.previous == page.shapes
                && page.previous_sources == page.sources
                && page.previous_material == Some(key)
            {
                continue;
            }
            if scene_changed || page.previous_sources != page.sources {
                for &(source, slot) in &page.sources {
                    let x = (source.left() - PAD).floor().max(0.0) as u32;
                    let y = (source.top() - PAD).floor().max(0.0) as u32;
                    let dx = (slot.left() + x as f32 - source.left()).round().max(0.0) as u32;
                    let dy = (slot.top() + y as f32 - source.top()).round().max(0.0) as u32;
                    let width = (source.right() + PAD).ceil().max(0.0) as u32;
                    let height = (source.bottom() + PAD).ceil().max(0.0) as u32;
                    let width = width
                        .min(self.scene.size[0])
                        .saturating_sub(x)
                        .min(SIDE.saturating_sub(dx));
                    let height = height
                        .min(self.scene.size[1])
                        .saturating_sub(y)
                        .min(SIDE.saturating_sub(dy));
                    if width == 0 || height == 0 {
                        continue;
                    }
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: self.scene.texture(),
                            mip_level: 0,
                            origin: wgpu::Origin3d { x, y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: page.backdrop.texture(),
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: dx, y: dy, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                self.renderer.regenerate_mips(&mut encoder, &page.backdrop);
            }
            self.renderer.render(
                &self.rs.queue,
                &mut encoder,
                &page.view,
                &page.backdrop,
                &Frame {
                    viewport: [SIDE as f32; 2],
                    scale_factor: 1.0,
                    backdrop_scale: 1.0,
                    scroll: 0.0,
                    shapes: &page.shapes,
                    params,
                },
            );
            page.previous.clone_from(&page.shapes);
            page.previous_sources.clone_from(&page.sources);
            page.previous_material = Some(key);
            changed = true;
        }
        if changed {
            self.rs.queue.submit([encoder.finish()]);
        }
        self.scene.finish(&output.textures_delta);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("glass-plate-count"), plate_count));
    }

    fn adapt_shape(
        &mut self,
        shape: &mut Shape,
        clip: Rect,
        pal: &crate::theme::Palette,
        page_index: &mut usize,
        count: &mut usize,
    ) {
        if let Shape::Vec(children) = shape {
            for child in children {
                self.adapt_shape(child, clip, pal, page_index, count);
            }
            return;
        }
        let Shape::Rect(r) = shape else { return };
        if !eligible(r, clip, pal) {
            return;
        }
        // Bound GPU memory on unusually complex dialogs; the remaining plates
        // keep their ordinary styled egui fill instead of disappearing.
        if *page_index >= 3 {
            return;
        }
        if self.pages.len() <= *page_index {
            self.add_page(pal.dark);
        }
        let mut slot = self.pages[*page_index].place(r.rect.size());
        if slot.is_none() {
            *page_index += 1;
            if *page_index >= 3 {
                return;
            }
            if self.pages.len() <= *page_index {
                self.add_page(pal.dark);
            }
            slot = self.pages[*page_index].place(r.rect.size());
        }
        let Some(slot) = slot else { return };
        let radius = r
            .corner_radius
            .nw
            .max(r.corner_radius.ne)
            .max(r.corner_radius.sw)
            .max(r.corner_radius.se) as f32;
        let [red, green, blue, _] = r.fill.to_srgba_unmultiplied();
        let glass = GlassShape::new([slot.center().x, slot.center().y], [slot.width() / 2.0, slot.height() / 2.0], radius)
            .appearance(if pal.dark { 1.0 } else { 0.0 })
            .group(self.pages[*page_index].shapes.len() as u32)
            .tint([red as f32 / 255.0, green as f32 / 255.0, blue as f32 / 255.0], SHEET_TINT);
        let page = &mut self.pages[*page_index];
        page.shapes.push(glass);
        page.sources.push((r.rect, slot));
        let uv = Rect::from_min_max(
            pos2(slot.left() / SIDE as f32, slot.top() / SIDE as f32),
            pos2(slot.right() / SIDE as f32, slot.bottom() / SIDE as f32),
        );
        *shape = texture_plate(r.clone(), page.id, uv);
        *count += 1;
    }
}

impl Drop for GlassAtlas {
    fn drop(&mut self) {
        self.clear_pages();
    }
}

/// Keep textured fill and plain stroke in separate meshes. Epaint's textured
/// rectangle path otherwise tries to add colored vertices to the native texture.
fn texture_plate(mut plate: egui::epaint::RectShape, texture: TextureId, uv: Rect) -> Shape {
    let alpha = plate.fill.a();
    plate.fill = Color32::from_white_alpha(if alpha == 0 { 255 } else { alpha });
    plate.brush = egui::epaint::RectShape::filled(plate.rect, CornerRadius::ZERO, Color32::WHITE)
        .with_texture(texture, uv)
        .brush;
    let stroke = plate.stroke;
    plate.stroke = egui::Stroke::NONE;
    if stroke.width > 0.0 {
        let border = egui::epaint::RectShape::stroke(
            plate.rect,
            plate.corner_radius,
            stroke,
            plate.stroke_kind,
        );
        Shape::Vec(vec![Shape::Rect(plate), Shape::Rect(border)])
    } else {
        Shape::Rect(plate)
    }
}

fn material_params(theme: Theme) -> embun::render_glass::GlassParams {
    let mut params = theme.glass();
    // Preserve action-label contrast when the window loses focus. Embun's
    // plain unfocused material removes prominence; our amber actions retain
    // their semantic tint while the surrounding glass still frosts more.
    if params.tint.is_none() {
        params.tint = Theme {
            focused: true,
            ..theme
        }
        .glass()
        .tint;
    }
    params
}

/// A floating sheet: rounded, filled with exactly the sheet colour, and on screen.
fn eligible(r: &egui::epaint::RectShape, clip: Rect, pal: &crate::theme::Palette) -> bool {
    let size = r.rect.size();
    r.brush.is_none()
        && r.blur_width == 0.0
        && r.fill == pal.sheet()
        && r.corner_radius != CornerRadius::ZERO
        && r.rect.is_finite()
        && r.rect.intersects(clip)
        && size.x >= 12.0
        && size.y >= 12.0
        && size.x + PAD * 2.0 <= SIDE as f32
        && size.y + PAD * 2.0 <= SIDE as f32
}

#[cfg(test)]
fn mineral_pixels(dark: bool) -> Vec<u8> {
    let (a, b, c) = if dark {
        ([23.0, 41.0, 34.0], [57.0, 56.0, 40.0], [32.0, 58.0, 48.0])
    } else {
        (
            [185.0, 201.0, 190.0],
            [231.0, 223.0, 201.0],
            [168.0, 196.0, 185.0],
        )
    };
    let mut pixels = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256 {
        for x in 0..256 {
            let t = (x as f32 + y as f32 * 0.45) / 370.0;
            let (from, to, t) = if t < 0.55 {
                (a, b, t / 0.55)
            } else {
                (b, c, (t - 0.55) / 0.45)
            };
            for i in 0..3 {
                pixels.push((from[i] + (to[i] - from[i]) * t.clamp(0.0, 1.0)) as u8);
            }
            pixels.push(255);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_floating_sheets_become_glass() {
        for pal in [crate::theme::Palette::day(), crate::theme::Palette::night()] {
            let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(1440.0, 900.0));
            let rect = Rect::from_min_size(pos2(20.0, 20.0), egui::vec2(220.0, 160.0));
            let menu = egui::epaint::RectShape::filled(rect, 12, pal.sheet());
            assert!(eligible(&menu, clip, &pal), "a rounded sheet is a menu or popup");
            for (fill, what) in [(pal.surface, "a button"), (pal.sidebar, "the sidebar"), (pal.content, "the canvas"), (pal.accent, "the primary button")] {
                let other = egui::epaint::RectShape::filled(rect, 12, fill);
                assert!(!eligible(&other, clip, &pal), "{what} must stay flat");
            }
            let square = egui::epaint::RectShape::filled(rect, 0, pal.sheet());
            assert!(!eligible(&square, clip, &pal), "square fills are data or panels, not sheets");
            let image = menu.clone().with_texture(TextureId::Managed(8), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
            assert!(!eligible(&image, clip, &pal), "images keep their pixels");
            let hidden = clip.translate(egui::vec2(2000.0, 0.0));
            assert!(!eligible(&menu, hidden, &pal), "do not spend atlas slots outside the clip");
        }
    }

    #[test]
    fn textured_control_tessellates_fill_and_boundary_without_mixing_textures() {
        let ctx = egui::Context::default();
        let mut initial = ctx.run_ui(Default::default(), |_| {});
        initial.textures_delta.clear();
        let rect = Rect::from_min_size(pos2(8.0, 8.0), egui::vec2(100.0, 36.0));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(0.5, 0.5));
        let shape = texture_plate(
            egui::epaint::RectShape::new(
                rect,
                8,
                Color32::WHITE,
                egui::Stroke::new(1.0, Color32::BLACK),
                egui::StrokeKind::Inside,
            ),
            TextureId::User(9),
            uv,
        );
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 200.0));
        let meshes = ctx.tessellate(vec![egui::epaint::ClippedShape::new(clip, shape)], 1.0);
        let textures: Vec<_> = meshes
            .iter()
            .filter_map(|m| match &m.primitive {
                egui::epaint::Primitive::Mesh(mesh) => Some(mesh.texture_id),
                _ => None,
            })
            .collect();
        assert_eq!(textures, vec![TextureId::User(9), TextureId::default()]);
        assert!(meshes.iter().all(|m| match &m.primitive {
            egui::epaint::Primitive::Mesh(mesh) => !mesh.vertices.is_empty(),
            _ => false,
        }));
    }

    #[test]
    fn measured_glass_changes_pixels_at_clear_and_tinted_levels() {
        // Uses a real GPU (or wgpu software adapter), not mocked shader inputs.
        let (device, queue) = crate::runtime::rt().block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&Default::default())
                .await
                .expect("GPU adapter for glass verification");
            adapter
                .request_device(&Default::default())
                .await
                .expect("glass GPU device")
        });
        let renderer = GlassRenderer::new(&device, Backdrop::FORMAT);
        let backdrop = renderer.create_backdrop(&device, &queue, &mineral_pixels(false), 256, 256);
        let shapes = [GlassShape::new([100.0, 70.0], [80.0, 40.0], 16.0)];
        let pixels = |level| {
            let theme = Theme {
                glass_level: level,
                ..Default::default()
            };
            embun::render_glass::offscreen::render_rgba(
                &device,
                &queue,
                &renderer,
                &backdrop,
                &Frame {
                    viewport: [200.0, 140.0],
                    scale_factor: 1.0,
                    backdrop_scale: 1.0,
                    scroll: 0.0,
                    shapes: &shapes,
                    params: theme.glass(),
                },
            )
            .0
        };
        let clear = pixels(0.0);
        let tinted = pixels(1.0);
        let center = (70 * 200 + 100) * 4;
        let delta: u32 = (0..3)
            .map(|i| clear[center + i].abs_diff(tinted[center + i]) as u32)
            .sum();
        assert!(
            delta > 15,
            "level must change the rendered body, RGB delta={delta}"
        );
        assert_eq!(
            &clear[0..4],
            &tinted[0..4],
            "the backdrop outside glass must stay unchanged"
        );
        assert!(clear.chunks_exact(4).all(|p| p[3] == 255));
        let primary = [shapes[0].tint([148.0 / 255.0, 86.0 / 255.0, 0.0], 1.0)];
        let params = material_params(Theme {
            focused: false,
            ..Default::default()
        });
        let unfocused = embun::render_glass::offscreen::render_rgba(
            &device,
            &queue,
            &renderer,
            &backdrop,
            &Frame {
                viewport: [200.0, 140.0],
                scale_factor: 1.0,
                backdrop_scale: 1.0,
                scroll: 0.0,
                shapes: &primary,
                params,
            },
        )
        .0;
        let linear = |v: u8| {
            let x = v as f32 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance = 0.2126 * linear(unfocused[center])
            + 0.7152 * linear(unfocused[center + 1])
            + 0.0722 * linear(unfocused[center + 2]);
        let contrast = 1.05 / (luminance + 0.05);
        assert!(
            contrast >= 4.5,
            "unfocused primary labels must remain readable, contrast={contrast}"
        );
    }
    #[test]
    fn glass_samples_each_screen_location_and_refreshes_changed_content() {
        let rs = crate::runtime::rt().block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&Default::default())
                .await
                .expect("GPU adapter");
            let (device, queue) = adapter
                .request_device(&Default::default())
                .await
                .expect("GPU device");
            let renderer =
                eframe::egui_wgpu::Renderer::new(&device, ATLAS_FORMAT, Default::default());
            RenderState {
                instance,
                available_adapters: vec![adapter.clone()],
                adapter,
                device,
                queue,
                target_format: ATLAS_FORMAT,
                renderer: std::sync::Arc::new(egui::mutex::RwLock::new(renderer)),
            }
        });
        let mut atlas = GlassAtlas::new(rs);
        let ctx = egui::Context::default();
        let pal = super::super::p();
        let mut draw = |left: Color32, right: Color32| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        egui::vec2(300.0, 140.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.label("Load the font atlas");
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(150.0, 140.0)),
                        0,
                        left,
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(150.0, 0.0), pos2(300.0, 140.0)),
                        0,
                        right,
                    );
                    for x in [20.0, 180.0] {
                        ui.painter().rect_filled(
                            Rect::from_min_size(pos2(x, 30.0), egui::vec2(100.0, 40.0)),
                            18,
                            pal.sheet(),
                        );
                    }
                },
            );
            atlas.paint(&ctx, &mut output);
            output.textures_delta.clear(); // Uploaded by the adapter; this test has no main presentation pass.
            assert_eq!(atlas.pages[0].shapes.len(), 2);
            let page = &atlas.pages[0];
            let theme = Theme {
                appearance: if pal.dark {
                    Appearance::Dark
                } else {
                    Appearance::Light
                },
                glass_level: 0.0,
                ..Default::default()
            };
            let bytes = embun::render_glass::offscreen::render_rgba_as(
                &atlas.rs.device,
                &atlas.rs.queue,
                &atlas.renderer,
                &page.backdrop,
                &Frame {
                    viewport: [256.0, 80.0],
                    scale_factor: 1.0,
                    backdrop_scale: 1.0,
                    scroll: 0.0,
                    shapes: &page.shapes,
                    params: material_params(theme),
                },
                ATLAS_FORMAT,
            )
            .0;
            let sample = |x: usize| {
                let i = (28 * 256 + x) * 4;
                [bytes[i], bytes[i + 1], bytes[i + 2]]
            };
            (sample(58), sample(174))
        };
        let red = Color32::from_rgb(210, 30, 30);
        let blue = Color32::from_rgb(30, 30, 210);
        let (a, b) = draw(red, blue);
        assert!(
            a[0] > a[2] + 10 && b[2] > b[0] + 10,
            "separate plates must sample their own screen position: {a:?} {b:?}"
        );
        let (a, b) = draw(blue, red);
        assert!(
            a[2] > a[0] + 10 && b[0] > b[2] + 10,
            "changing the content must invalidate the scene and frost cache: {a:?} {b:?}"
        );
        drop(draw);
        configure(&ctx, true);
        let mut opaque = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    egui::vec2(300.0, 140.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.painter().rect_filled(
                    Rect::from_min_size(pos2(20.0, 30.0), egui::vec2(100.0, 40.0)),
                    18,
                    pal.sheet(),
                );
            },
        );
        let original = opaque.shapes.clone();
        atlas.paint(&ctx, &mut opaque);
        opaque.textures_delta.clear();
        assert_eq!(
            opaque.shapes, original,
            "reduced transparency must preserve the opaque shapes without glass textures"
        );
    }
}
