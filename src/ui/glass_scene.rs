//! Scene behind the glass. The normal egui draw still owns the visible text and controls.
use super::*;
use eframe::egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};

pub(super) struct Scene {
    renderer: Renderer,
    texture: Option<wgpu::Texture>,
    pub size: [u32; 2],
    previous: Vec<egui::epaint::ClippedShape>,
}

impl Scene {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            renderer: Renderer::new(device, Backdrop::FORMAT, RendererOptions::default()),
            texture: None,
            size: [0, 0],
            previous: Vec::new(),
        }
    }
    pub fn update_textures(&mut self, rs: &RenderState, delta: &egui::TexturesDelta) {
        for (id, images) in &delta.set {
            for image in images {
                self.renderer
                    .update_texture(&rs.device, &rs.queue, *id, image);
            }
        }
    }
    pub fn finish(&mut self, delta: &egui::TexturesDelta) {
        for id in &delta.free {
            self.renderer.free_texture(id);
        }
    }
    pub fn texture(&self) -> &wgpu::Texture {
        self.texture.as_ref().expect("prepared glass scene")
    }
    pub fn render(
        &mut self,
        rs: &RenderState,
        ctx: &egui::Context,
        output: &egui::FullOutput,
        encoder: &mut wgpu::CommandEncoder,
        pal: &crate::theme::Palette,
    ) -> bool {
        let viewport = ctx.content_rect();
        let size = [
            viewport.width().ceil().max(1.0) as u32,
            viewport.height().ceil().max(1.0) as u32,
        ];
        // Bound the GPU allocation for unusually large windows. The opaque UI remains usable.
        if size
            .iter()
            .any(|&n| n > rs.device.limits().max_texture_dimension_2d)
        {
            return false;
        }
        let changed_size = self.size != size;
        if changed_size {
            self.texture = Some(rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Strata live glass scene"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Backdrop::FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }));
            self.size = size;
        }
        let shapes = underlay(&output.shapes, viewport, pal);
        if !changed_size && shapes == self.previous && output.textures_delta.set.is_empty() {
            return false;
        }
        self.previous.clone_from(&shapes);
        let mut jobs = ctx.tessellate(shapes, 1.0);
        // Native GPU callbacks belong to the main pass; managed images and text are safe here.
        jobs.retain(|job| matches!(&job.primitive, egui::epaint::Primitive::Mesh(mesh) if matches!(mesh.texture_id, TextureId::Managed(_))));
        let screen = ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: 1.0,
        };
        let commands = self
            .renderer
            .update_buffers(&rs.device, &rs.queue, encoder, &jobs, &screen);
        if !commands.is_empty() {
            rs.queue.submit(commands);
        }
        let view = self.texture().create_view(&Default::default());
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Strata glass scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        self.renderer.render(&mut pass, &jobs, &screen);
        true
    }
}

/// Keep content beneath a plate, omit the plate and its own foreground. A later popup
/// therefore samples the previously drawn editor/grid, rather than refracting its own label.
fn underlay(
    shapes: &[egui::epaint::ClippedShape],
    viewport: Rect,
    pal: &crate::theme::Palette,
) -> Vec<egui::epaint::ClippedShape> {
    let mut result = vec![egui::epaint::ClippedShape::new(
        viewport,
        wallpaper(viewport, pal.dark),
    )];
    let mut covers = Vec::new();
    fn visit(
        shape: &Shape,
        clip: Rect,
        viewport: Rect,
        pal: &crate::theme::Palette,
        covers: &mut Vec<Rect>,
        out: &mut Vec<egui::epaint::ClippedShape>,
    ) {
        if let Shape::Vec(children) = shape {
            for child in children {
                visit(child, clip, viewport, pal, covers, out);
            }
            return;
        }
        if let Shape::Rect(rect) = shape {
            if eligible(rect, clip, pal) {
                covers.push(rect.rect);
                return;
            }
            if rect.rect.contains_rect(viewport.shrink(1.0))
                && [pal.window, pal.ground, pal.content].contains(&rect.fill)
            {
                return;
            }
        }
        let bounds = shape.visual_bounding_rect().intersect(clip);
        if covers.iter().any(|r| r.contains_rect(bounds)) {
            return;
        }
        out.push(egui::epaint::ClippedShape::new(clip, shape.clone()));
    }
    for entry in shapes {
        visit(
            &entry.shape,
            entry.clip_rect.intersect(viewport),
            viewport,
            pal,
            &mut covers,
            &mut result,
        );
    }
    result
}

/// Neutral environment behind the app content, so a sheet over an empty area still has something to frost.
fn wallpaper(rect: Rect, dark: bool) -> Shape {
    let mut mesh = egui::Mesh::default();
    let stops: &[(f32, u8)] = if dark {
        &[
            (0.0, 12),
            (0.18, 42),
            (0.32, 100),
            (0.39, 24),
            (0.66, 10),
            (0.82, 56),
            (1.0, 17),
        ]
    } else {
        &[
            (0.0, 213),
            (0.18, 238),
            (0.32, 255),
            (0.39, 199),
            (0.66, 231),
            (0.82, 250),
            (1.0, 218),
        ]
    };
    for &(t, value) in stops {
        let x = rect.left() + t * rect.width();
        let color = Color32::from_rgb(value, value, value.saturating_add(if dark { 2 } else { 3 }));
        mesh.colored_vertex(pos2(x - rect.height() * 0.15, rect.top()), color);
        mesh.colored_vertex(pos2(x + rect.height() * 0.15, rect.bottom()), color);
    }
    // Extend the first/last strip outside the viewport so the environment fills every edge.
    mesh.vertices[0].pos.x = rect.left();
    mesh.vertices[1].pos.x = rect.left();
    let n = mesh.vertices.len();
    mesh.vertices[n - 2].pos.x = rect.right();
    mesh.vertices[n - 1].pos.x = rect.right();
    for i in 0..stops.len() - 1 {
        let a = (i * 2) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_samples_prior_content_and_excludes_its_own_foreground() {
        let pal = crate::theme::Palette::night();
        let viewport = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 200.0));
        let marker = Shape::rect_filled(
            Rect::from_min_size(pos2(60.0, 60.0), egui::vec2(8.0, 8.0)),
            0,
            Color32::RED,
        );
        let plate = Shape::rect_filled(
            Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(140.0, 80.0)),
            18,
            pal.sheet(),
        );
        let foreground = Shape::rect_filled(
            Rect::from_min_size(pos2(80.0, 60.0), egui::vec2(8.0, 8.0)),
            0,
            Color32::BLUE,
        );
        let input = [marker.clone(), plate.clone(), foreground.clone()]
            .map(|shape| egui::epaint::ClippedShape::new(viewport, shape));
        let result = underlay(&input, viewport, &pal);
        assert!(
            result.iter().any(|s| s.shape == marker),
            "content drawn before the popup must be sampled"
        );
        assert!(
            !result
                .iter()
                .any(|s| s.shape == plate || s.shape == foreground),
            "the popup must not frost its own surface or label"
        );
    }
}
