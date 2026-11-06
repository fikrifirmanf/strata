//! Motion tokens: four durations and one decelerating curve. Things settle like stone: they
//! slow down and stop, and never bounce. Everything stays under 280 ms so it never slows you down.

use egui::{Context, Id};

/// Cursor and hover moves.
pub const FAST: f64 = 0.09;
/// Sections unfolding, tab ink sliding, panels crossfading.
pub const SETTLE: f64 = 0.22;
/// Sheets and dialogs entering.
pub const ENTER: f64 = 0.28;

/// Elapsed time as a 0..=1 fraction of `duration`.
pub fn progress(elapsed: f64, duration: f64) -> f32 {
    if duration <= 0.0 { 1.0 } else { (elapsed / duration).clamp(0.0, 1.0) as f32 }
}

/// Cubic ease-out, close to the design's cubic-bezier(.2, .8, .2, 1).
pub fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Linear 0..=1 progress since `id` started showing. Something counts as newly shown when it
/// was not drawn on the previous frame, so reopening a dialog plays its entrance again.
pub fn entered(ctx: &Context, id: Id, duration: f64) -> f32 {
    let key = id.with("motion-entered");
    let (now, frame) = (ctx.input(|i| i.time), ctx.cumulative_frame_nr());
    let start = ctx.data_mut(|d| {
        let (start, last) = d.get_temp::<(f64, u64)>(key).unwrap_or((now, frame));
        let start = if last + 1 < frame { now } else { start };
        d.insert_temp(key, (start, frame));
        start
    });
    let t = progress(now - start, duration);
    if t < 1.0 {
        ctx.request_repaint();
    }
    t
}

/// A value that eases toward `target`, for tab ink, thumbs and other things that travel.
pub fn glide(ctx: &Context, id: Id, target: f32, duration: f64) -> f32 {
    ctx.animate_value_with_time(id, target, duration as f32)
}

/// Shows `add` in a section that unfolds and folds over `SETTLE` instead of snapping.
pub fn unfold<R>(ui: &mut egui::Ui, id: Id, open: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    state.set_open(open);
    state.show_body_unindented(ui, add).map(|r| r.inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_starts_at_zero_ends_at_one_and_never_overshoots() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        let samples: Vec<f32> = (0..=20).map(|i| ease_out(i as f32 / 20.0)).collect();
        assert!(samples.windows(2).all(|w| w[1] >= w[0]), "curve must be monotonic");
        assert!(samples.iter().all(|v| *v <= 1.0), "curve must not bounce past 1");
        assert!(ease_out(0.5) > 0.5, "an ease-out covers most of the distance early");
    }

    #[test]
    fn progress_clamps_to_the_unit_range() {
        assert_eq!(progress(-1.0, FAST), 0.0);
        assert_eq!(progress(10.0, FAST), 1.0);
        assert_eq!(progress(0.0, 0.0), 1.0);
    }
}
