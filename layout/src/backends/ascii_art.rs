/*!
ASCII rendering backend that accepts draw calls and produces a simple ASCII art
representation as a String.

Notes:
- Coordinates are floating-point; they are rounded to the nearest grid cell.
- The Y axis grows downward, similar to typical screen coordinates.
- Colors, stroke width, and most style attributes are ignored for ASCII.
- Clip regions are recorded but not applied (ASCII backend ignores clipping).
*/

use crate::core::format::{ClipHandle, RenderBackend};
use crate::core::geometry::Point;
use crate::core::style::StyleAttr;

#[derive(Debug)]
pub struct ASCIIWriter {
    grid: Vec<Vec<char>>,
    width: usize,
    height: usize,
    clips: Vec<(Point, Point, usize)>, // (top-left, size, rounded_px) - not applied
}

impl ASCIIWriter {
    pub fn new() -> Self {
        Self {
            grid: Vec::new(),
            width: 0,
            height: 0,
            clips: Vec::new(),
        }
    }

    pub fn finalize(&self) -> String {
        let mut out = String::new();
        for row in &self.grid {
            // Trim trailing spaces for nicer output.
            let mut end = row.len();
            while end > 0 && row[end - 1] == ' ' {
                end -= 1;
            }
            let line: String = row[..end].iter().collect();
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    fn ensure_size(&mut self, x: usize, y: usize) {
        if y >= self.height {
            let new_height = y + 1;
            let fill_width = self.width.max(1);
            self.grid.resize_with(new_height, || vec![' '; fill_width]);
            self.height = new_height;
        }
        if x >= self.width {
            let new_width = x + 1;
            for row in &mut self.grid {
                row.resize(new_width, ' ');
            }
            self.width = new_width;
        }
    }

    fn to_ixy(&self, p: Point) -> (isize, isize) {
        (p.x.round() as isize, p.y.round() as isize)
    }

    fn clamp_nonneg(ix: isize, iy: isize) -> Option<(usize, usize)> {
        if ix < 0 || iy < 0 {
            None
        } else {
            Some((ix as usize, iy as usize))
        }
    }

    fn set(&mut self, ix: isize, iy: isize, ch: char) {
        if let Some((x, y)) = Self::clamp_nonneg(ix, iy) {
            self.ensure_size(x, y);
            self.grid[y][x] = ch;
        }
    }

    fn draw_hline(&mut self, x0: isize, x1: isize, y: isize, ch: char) {
        let (mut a, mut b) = (x0.min(x1), x0.max(x1));
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        for x in a..=b {
            self.set(x, y, ch);
        }
    }

    fn draw_vline(&mut self, x: isize, y0: isize, y1: isize, ch: char) {
        let (mut a, mut b) = (y0.min(y1), y0.max(y1));
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        for y in a..=b {
            self.set(x, y, ch);
        }
    }

    fn draw_line_segment(&mut self, p0: Point, p1: Point, ch: char) {
        let (mut x0, mut y0) = self.to_ixy(p0);
        let (x1, y1) = self.to_ixy(p1);

        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set(x0, y0, ch);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn draw_polyline(&mut self, anchors: &[Point], ch: char) {
        for i in 1..anchors.len() {
            self.draw_line_segment(anchors[i - 1], anchors[i], ch);
        }
    }

    fn rect_fill(&mut self, top_left: Point, size: Point, fill: char) {
        let (ix, iy) = self.to_ixy(top_left);
        let w = size.x.round().max(0.0) as isize;
        let h = size.y.round().max(0.0) as isize;
        for yy in 0..h {
            for xx in 0..w {
                self.set(ix + xx, iy + yy, fill);
            }
        }
    }

    fn rect_outline(&mut self, top_left: Point, size: Point) {
        let (ix, iy) = self.to_ixy(top_left);
        let w = size.x.round().max(0.0) as isize;
        let h = size.y.round().max(0.0) as isize;

        if w <= 0 || h <= 0 {
            return;
        }

        // Corners
        self.set(ix, iy, '+');
        self.set(ix + w - 1, iy, '+');
        self.set(ix, iy + h - 1, '+');
        self.set(ix + w - 1, iy + h - 1, '+');

        // Edges
        if w > 2 {
            self.draw_hline(ix + 1, ix + w - 2, iy, '-');
            self.draw_hline(ix + 1, ix + w - 2, iy + h - 1, '-');
        }
        if h > 2 {
            self.draw_vline(ix, iy + 1, iy + h - 2, '|');
            self.draw_vline(ix + w - 1, iy + 1, iy + h - 2, '|');
        }
    }

    fn ellipse_outline(&mut self, center: Point, size: Point, ch: char) {
        let a = (size.x / 2.0).max(0.0);
        let b = (size.y / 2.0).max(0.0);
        if a <= 0.0 || b <= 0.0 {
            return;
        }
        let cx = center.x;
        let cy = center.y;
        // Sample around the ellipse.
        let steps = 72; // 5 degrees per step
        for i in 0..steps {
            let t = (i as f64) * ((2.0 * std::f64::consts::PI) / steps as f64);
            let x = cx + a * t.cos();
            let y = cy + b * t.sin();
            let (ix, iy) = self.to_ixy(Point::new(x, y));
            self.set(ix, iy, ch);
        }
    }

    fn ellipse_fill(&mut self, center: Point, size: Point, ch: char) {
        let a = (size.x / 2.0).max(0.0);
        let b = (size.y / 2.0).max(0.0);
        if a <= 0.0 || b <= 0.0 {
            return;
        }
        let cy0 = (center.y - b).floor() as isize;
        let cy1 = (center.y + b).ceil() as isize;

        for iy in cy0..=cy1 {
            // Compute span width using ellipse equation x = a * sqrt(1 - (y^2 / b^2))
            let yy = iy as f64 + 0.0;
            let dy = yy - center.y;
            let inside = 1.0 - (dy * dy) / (b * b);
            if inside >= 0.0 {
                let span = a * inside.sqrt();
                let x0 = (center.x - span).floor() as isize;
                let x1 = (center.x + span).ceil() as isize;
                for ix in x0..=x1 {
                    self.set(ix, iy, ch);
                }
            }
        }
    }

    fn text_at_center(&mut self, center: Point, text: &str) {
        let lines: Vec<&str> = if text.is_empty() {
            vec![""]
        } else {
            text.lines().collect()
        };
        let (cx, cy) = self.to_ixy(center);
        let n = lines.len() as isize;
        let start_y = cy - (n - 1) / 2;
        for (i, line) in lines.iter().enumerate() {
            let line_len = line.chars().count() as isize;
            let start_x = cx - line_len / 2;
            let y = start_y + i as isize;
            for (j, ch) in line.chars().enumerate() {
                self.set(start_x + j as isize, y, ch);
            }
        }
    }

    fn head_char(dx: f64, dy: f64) -> char {
        if dx.abs() >= dy.abs() {
            if dx >= 0.0 {
                '>'
            } else {
                '<'
            }
        } else if dy >= 0.0 {
            'v'
        } else {
            '^'
        }
    }
}

impl Default for ASCIIWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBackend for ASCIIWriter {
    fn draw_rect(
        &mut self,
        xy: Point,
        size: Point,
        look: &StyleAttr,
        _properties: Option<String>,
        _clip: Option<ClipHandle>,
    ) {
        // Fill if requested, then outline.
        if look.fill_color.is_some() {
            self.rect_fill(xy, size, '.');
        }
        self.rect_outline(xy, size);
    }

    fn draw_line(
        &mut self,
        start: Point,
        stop: Point,
        _look: &StyleAttr,
        _properties: Option<String>,
    ) {
        // Use a generic line character.
        self.draw_line_segment(start, stop, '*');
    }

    fn draw_circle(
        &mut self,
        xy: Point,
        size: Point,
        look: &StyleAttr,
        _properties: Option<String>,
    ) {
        // Fill then outline.
        if look.fill_color.is_some() {
            self.ellipse_fill(xy, size, '.');
        }
        self.ellipse_outline(xy, size, 'o');
    }

    fn draw_text(&mut self, xy: Point, text: &str, _look: &StyleAttr) {
        self.text_at_center(xy, text);
    }

    fn draw_arrow(
        &mut self,
        path: &[(Point, Point)],
        dashed: bool,
        head: (bool, bool),
        _look: &StyleAttr,
        _properties: Option<String>,
        text: &str,
    ) {
        if path.is_empty() {
            return;
        }

        // Extract the anchor sequence (the first item of each tuple).
        let mut anchors: Vec<Point> = Vec::with_capacity(path.len());
        for (a, _) in path.iter() {
            anchors.push(*a);
        }

        // Draw the polyline connecting anchors. If dashed, use '.' else '*'.
        let seg_char = if dashed { '.' } else { '*' };
        self.draw_polyline(&anchors, seg_char);

        // Draw arrow heads at start/end if requested.
        if anchors.len() >= 2 {
            let n = anchors.len();
            if head.0 {
                let dir = anchors[1].sub(anchors[0]);
                let hc = Self::head_char(dir.x, dir.y);
                let (ix, iy) = self.to_ixy(anchors[0]);
                self.set(ix, iy, hc);
            }
            if head.1 {
                let dir = anchors[n - 1].sub(anchors[n - 2]);
                let hc = Self::head_char(dir.x, dir.y);
                let (ix, iy) = self.to_ixy(anchors[n - 1]);
                self.set(ix, iy, hc);
            }
        }

        // Place text roughly at the midpoint anchor, centered.
        if !text.is_empty() {
            let mid = anchors[anchors.len() / 2];
            self.text_at_center(mid, text);
        }
    }

    fn create_clip(
        &mut self,
        xy: Point,
        size: Point,
        rounded_px: usize,
    ) -> ClipHandle {
        self.clips.push((xy, size, rounded_px));
        self.clips.len() - 1
    }
}
