//! Geometry preview for PostGIS / spatial columns: parses WKB (hex, with EWKB SRID) and WKT,
//! then draws the shape fitted to a small canvas.

use super::p;
use egui::{pos2, vec2, Color32, CornerRadius, FontId, Pos2, Sense, Stroke, Ui};

#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Point(f64, f64),
    Line(Vec<(f64, f64)>),
    Polygon(Vec<Vec<(f64, f64)>>),
    Collection(Vec<Geometry>),
}

impl Geometry {
    fn points(&self, out: &mut Vec<(f64, f64)>) {
        match self {
            Geometry::Point(x, y) => out.push((*x, *y)),
            Geometry::Line(l) => out.extend(l),
            Geometry::Polygon(rings) => rings.iter().for_each(|r| out.extend(r)),
            Geometry::Collection(g) => g.iter().for_each(|g| g.points(out)),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Geometry::Point(..) => "Point",
            Geometry::Line(_) => "LineString",
            Geometry::Polygon(_) => "Polygon",
            Geometry::Collection(_) => "Collection",
        }
    }

    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut pts = Vec::new();
        self.points(&mut pts);
        let first = *pts.first()?;
        Some(pts.iter().fold((first.0, first.1, first.0, first.1), |(a, b, c, d), (x, y)| (a.min(*x), b.min(*y), c.max(*x), d.max(*y))))
    }
}

/// A short label for a grid cell: "Point 13.40, 52.52" or "Polygon · 5 vertices".
pub fn summary(g: &Geometry) -> String {
    let mut pts = Vec::new();
    g.points(&mut pts);
    match g {
        Geometry::Point(x, y) => format!("Point {x:.5}, {y:.5}"),
        Geometry::Collection(items) => format!("{} · {} parts", g.kind(), items.len()),
        _ => format!("{} · {} vertices", g.kind(), pts.len()),
    }
}

/// Parses EWKB hex (PostGIS text output), plain WKB hex, or WKT. Returns (SRID, geometry).
pub fn parse(s: &str) -> Option<(Option<u32>, Geometry)> {
    let t = s.trim();
    if t.len() >= 10 && t.len() % 2 == 0 && t.bytes().all(|b| b.is_ascii_hexdigit()) && (t.starts_with("00") || t.starts_with("01")) {
        let bytes = hex::decode(t).ok()?;
        let mut r = Reader { b: &bytes, i: 0, le: true };
        let mut srid = None;
        let g = r.geometry(&mut srid)?;
        return Some((srid, g));
    }
    let (srid, body) = match t.strip_prefix("SRID=").and_then(|rest| rest.split_once(';')) {
        Some((n, body)) => (n.parse().ok(), body),
        None => (None, t),
    };
    let mut w = Wkt { s: body.as_bytes(), i: 0 };
    let g = w.geometry()?;
    Some((srid, g))
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
    le: bool,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.i..self.i + N)?;
        self.i += N;
        s.try_into().ok()
    }
    fn u32(&mut self) -> Option<u32> {
        let a = self.take::<4>()?;
        Some(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }
    fn f64(&mut self) -> Option<f64> {
        let a = self.take::<8>()?;
        Some(if self.le { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) })
    }
    fn point(&mut self, dims: usize) -> Option<(f64, f64)> {
        let x = self.f64()?;
        let y = self.f64()?;
        for _ in 2..dims {
            self.f64()?;
        }
        Some((x, y))
    }
    fn points(&mut self, dims: usize) -> Option<Vec<(f64, f64)>> {
        let n = self.u32()? as usize;
        if n > 5_000_000 {
            return None;
        }
        (0..n).map(|_| self.point(dims)).collect()
    }
    fn geometry(&mut self, srid: &mut Option<u32>) -> Option<Geometry> {
        self.le = *self.b.get(self.i)? == 1;
        self.i += 1;
        let raw = self.u32()?;
        // EWKB flags: Z 0x80000000, M 0x40000000, SRID 0x20000000. ISO WKB adds 1000/2000/3000.
        let mut dims = 2 + usize::from(raw & 0x8000_0000 != 0) + usize::from(raw & 0x4000_0000 != 0);
        if raw & 0x2000_0000 != 0 {
            *srid = Some(self.u32()?);
        }
        let mut ty = raw & 0x0FFF_FFFF;
        if ty > 1000 {
            dims += match ty / 1000 {
                1 | 2 => 1,
                3 => 2,
                _ => 0,
            };
            ty %= 1000;
        }
        Some(match ty {
            1 => {
                let (x, y) = self.point(dims)?;
                Geometry::Point(x, y)
            }
            2 => Geometry::Line(self.points(dims)?),
            3 => {
                let n = self.u32()? as usize;
                Geometry::Polygon((0..n).map(|_| self.points(dims)).collect::<Option<_>>()?)
            }
            4..=7 => {
                let n = self.u32()? as usize;
                let mut parts = Vec::with_capacity(n.min(100_000));
                for _ in 0..n {
                    let mut inner = None;
                    parts.push(self.geometry(&mut inner)?);
                }
                Geometry::Collection(parts)
            }
            _ => return None,
        })
    }
}

struct Wkt<'a> {
    s: &'a [u8],
    i: usize,
}

impl Wkt<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }
    fn word(&mut self) -> String {
        self.ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).to_ascii_uppercase()
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn number(&mut self) -> Option<f64> {
        self.ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'+' | b'e' | b'E')) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }
    fn coord(&mut self) -> Option<(f64, f64)> {
        let x = self.number()?;
        let y = self.number()?;
        // Skip Z / M.
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(c) if c.is_ascii_digit() || *c == b'-' => {
                    self.number()?;
                }
                _ => break,
            }
        }
        Some((x, y))
    }
    fn coords(&mut self) -> Option<Vec<(f64, f64)>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut v = Vec::new();
        loop {
            // MULTIPOINT may wrap each point in parentheses.
            let wrapped = self.eat(b'(');
            v.push(self.coord()?);
            if wrapped && !self.eat(b')') {
                return None;
            }
            if !self.eat(b',') {
                break;
            }
        }
        self.eat(b')').then_some(v)
    }
    fn rings(&mut self) -> Option<Vec<Vec<(f64, f64)>>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut v = Vec::new();
        loop {
            v.push(self.coords()?);
            if !self.eat(b',') {
                break;
            }
        }
        self.eat(b')').then_some(v)
    }
    fn geometry(&mut self) -> Option<Geometry> {
        let kind = self.word();
        let rest = self.word();
        if rest == "EMPTY" {
            return Some(Geometry::Collection(vec![]));
        }
        Some(match kind.as_str() {
            "POINT" => {
                self.eat(b'(').then_some(())?;
                let (x, y) = self.coord()?;
                self.eat(b')').then_some(())?;
                Geometry::Point(x, y)
            }
            "LINESTRING" => Geometry::Line(self.coords()?),
            "POLYGON" => Geometry::Polygon(self.rings()?),
            "MULTIPOINT" => Geometry::Collection(self.coords()?.into_iter().map(|(x, y)| Geometry::Point(x, y)).collect()),
            "MULTILINESTRING" => Geometry::Collection(self.rings()?.into_iter().map(Geometry::Line).collect()),
            "MULTIPOLYGON" => {
                self.eat(b'(').then_some(())?;
                let mut v = Vec::new();
                loop {
                    v.push(Geometry::Polygon(self.rings()?));
                    if !self.eat(b',') {
                        break;
                    }
                }
                self.eat(b')').then_some(())?;
                Geometry::Collection(v)
            }
            "GEOMETRYCOLLECTION" => {
                self.eat(b'(').then_some(())?;
                let mut v = Vec::new();
                loop {
                    v.push(self.geometry()?);
                    if !self.eat(b',') {
                        break;
                    }
                }
                self.eat(b')').then_some(())?;
                Geometry::Collection(v)
            }
            _ => return None,
        })
    }
}

/// Draws `g` fitted into a `height`-tall canvas the width of the available space.
pub fn show(ui: &mut Ui, g: &Geometry, srid: Option<u32>, height: f32) {
    let pal = p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), if pal.dark { Color32::from_black_alpha(80) } else { Color32::from_black_alpha(10) });
    let inner = rect.shrink(18.0);
    for i in 1..4 {
        let x = inner.left() + inner.width() * i as f32 / 4.0;
        let y = inner.top() + inner.height() * i as f32 / 4.0;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, pal.separator));
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, pal.separator));
    }
    let Some((x0, y0, x1, y1)) = g.bounds() else { return };
    let (w, h) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
    let scale = (inner.width() as f64 / w).min(inner.height() as f64 / h);
    let off = vec2(((inner.width() as f64 - w * scale) / 2.0) as f32, ((inner.height() as f64 - h * scale) / 2.0) as f32);
    let to = |(x, y): (f64, f64)| -> Pos2 {
        if x1 == x0 && y1 == y0 {
            return inner.center();
        }
        pos2(inner.left() + off.x + ((x - x0) * scale) as f32, inner.bottom() - off.y - ((y - y0) * scale) as f32)
    };
    draw(&painter, g, &to);
    let label = |v: f64| if v.abs() >= 1000.0 { format!("{v:.0}") } else { format!("{v:.4}") };
    painter.text(rect.left_bottom() + vec2(8.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("{}, {}", label(x0), label(y0)), FontId::monospace(10.0), pal.text_faint);
    painter.text(rect.right_top() + vec2(-8.0, 6.0), egui::Align2::RIGHT_TOP, format!("{}, {}", label(x1), label(y1)), FontId::monospace(10.0), pal.text_faint);
    if let Some(s) = srid {
        painter.text(rect.left_top() + vec2(8.0, 6.0), egui::Align2::LEFT_TOP, format!("SRID {s}{}", if s == 4326 { " · lon/lat" } else { "" }), FontId::proportional(10.5), pal.text_faint);
    }
}

fn draw(painter: &egui::Painter, g: &Geometry, to: &dyn Fn((f64, f64)) -> Pos2) {
    let pal = p();
    let stroke = Stroke::new(1.8, pal.accent2);
    match g {
        Geometry::Point(x, y) => {
            painter.circle_filled(to((*x, *y)), 4.5, pal.accent);
            painter.circle_stroke(to((*x, *y)), 8.0, Stroke::new(1.0, pal.accent.gamma_multiply(0.5)));
        }
        Geometry::Line(l) => {
            painter.add(egui::Shape::line(l.iter().map(|p| to(*p)).collect(), stroke));
        }
        Geometry::Polygon(rings) => {
            for (i, ring) in rings.iter().enumerate() {
                let pts: Vec<Pos2> = ring.iter().map(|p| to(*p)).collect();
                if i == 0 && pts.len() >= 3 {
                    let fill = Color32::from_rgba_unmultiplied(pal.accent2.r(), pal.accent2.g(), pal.accent2.b(), 40);
                    // Non-convex outlines are filled by a triangle fan; good enough for a preview.
                    let c = pts[0];
                    for w in pts.windows(2).skip(1) {
                        painter.add(egui::Shape::convex_polygon(vec![c, w[0], w[1]], fill, Stroke::NONE));
                    }
                }
                painter.add(egui::Shape::closed_line(pts, stroke));
            }
        }
        Geometry::Collection(items) => items.iter().for_each(|g| draw(painter, g, to)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_postgis_ewkb_point_with_srid() {
        // ST_SetSRID(ST_MakePoint(13.4, 52.52), 4326)
        let (srid, g) = parse("0101000020E6100000CDCCCCCCCCCC2A40C3F5285C8F424A40").unwrap();
        assert_eq!(srid, Some(4326));
        let Geometry::Point(x, y) = g else { panic!("{g:?}") };
        assert!((x - 13.4).abs() < 1e-9 && (y - 52.52).abs() < 1e-9);
    }

    #[test]
    fn parses_wkt_polygon_and_multipoint() {
        let (srid, g) = parse("SRID=3857;POLYGON((0 0, 4 0, 4 3, 0 3, 0 0))").unwrap();
        assert_eq!(srid, Some(3857));
        assert_eq!(g.bounds(), Some((0.0, 0.0, 4.0, 3.0)));
        let (_, m) = parse("MULTIPOINT((1 2), (3 4))").unwrap();
        assert_eq!(m, Geometry::Collection(vec![Geometry::Point(1.0, 2.0), Geometry::Point(3.0, 4.0)]));
        assert_eq!(summary(&m), "Collection · 2 parts");
    }

    #[test]
    fn rejects_ordinary_text_and_numbers() {
        assert!(parse("hello world").is_none());
        assert!(parse("0123").is_none());
        assert!(parse("deadbeef").is_none());
    }
}
