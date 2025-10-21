/*!
ASCII rendering backend that accepts draw calls and produces a simple ASCII art
representation as a String.

Notes:
- Coordinates are floating-point; they are rounded to the nearest grid cell.
- The Y axis grows downward, similar to typical screen coordinates.
- Colors, stroke width, and most style attributes are ignored for ASCII.
- Clip regions are recorded but not applied (ASCII backend ignores clipping).

Terminal vs Non-Terminal Behavior:
- Terminal output: Fills shapes with Unicode block characters (█, ●) when fill_color is specified
- Terminal colors: Uses ANSI escape codes to color the fill characters when use_colors is enabled
- Non-terminal output: Only draws outlines, no fill characters (useful for plain text files)

Usage Examples:
```rust
// Auto-detect terminal output (colors enabled if terminal detected)
let mut writer = ASCIIWriter::new();

// Force terminal mode with colors (fills with colored characters)
let mut terminal_writer = ASCIIWriter::new_with_terminal_setting(true);

// Force non-terminal mode (outline only, no colors)
let mut file_writer = ASCIIWriter::new_with_terminal_setting(false);

// Control color usage in terminal mode
let mut writer = ASCIIWriter::new_with_color_setting(true, false);
writer.set_use_colors(true); // Enable ANSI color codes for fill characters
```

Color Output Examples:
- Terminal with colors: `\x1b[31m█\x1b[0m` (red filled block)
- Terminal without colors: `█` (plain filled block)
- Non-terminal: `+--+\n|  |\n+--+` (outline only)
*/

use crate::core::format::{ClipHandle, RenderBackend};
use crate::core::geometry::Point;
use crate::core::style::StyleAttr;

// External crates for terminal detection and coloring
use atty;
use termcolor::Color;

#[derive(Debug)]
pub struct ASCIIWriter {
    grid: Vec<Vec<(char, Option<termcolor::Color>)>>, // char with optional color
    width: usize,
    height: usize,
    scale: f64, // pixels per cell (derived from font size)
    clips: Vec<(Point, Point, usize)>, // (top-left, size, rounded_px) - not applied
    is_terminal: bool, // whether output is targeted for terminal
    use_colors: bool,  // whether to use colors in terminal output
}

impl ASCIIWriter {
    pub fn new() -> Self {
        Self {
            grid: Vec::new(),
            width: 0,
            height: 0,
            scale: 20.0,
            clips: Vec::new(),
            is_terminal: atty::is(atty::Stream::Stdout),
            use_colors: atty::is(atty::Stream::Stdout),
        }
    }

    pub fn new_with_terminal_setting(is_terminal: bool) -> Self {
        Self {
            grid: Vec::new(),
            width: 0,
            height: 0,
            scale: 6.0,
            clips: Vec::new(),
            is_terminal,
            use_colors: is_terminal,
        }
    }

    pub fn new_with_color_setting(is_terminal: bool, use_colors: bool) -> Self {
        Self {
            grid: Vec::new(),
            width: 0,
            height: 0,
            scale: 6.0,
            clips: Vec::new(),
            is_terminal,
            use_colors,
        }
    }

    pub fn finalize(&self) -> String {
        if self.is_terminal && self.use_colors {
            self.finalize_with_colors()
        } else {
            self.finalize_plain()
        }
    }

    fn finalize_plain(&self) -> String {
        let mut out = String::new();
        for row in &self.grid {
            // Trim trailing spaces for nicer output.
            let mut end = row.len();
            while end > 0 && row[end - 1].0 == ' ' {
                end -= 1;
            }
            let line: String = row[..end].iter().map(|(ch, _)| *ch).collect();
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    fn finalize_with_colors(&self) -> String {
        let mut out = String::new();
        for row in &self.grid {
            // Trim trailing spaces for nicer output.
            let mut end = row.len();
            while end > 0 && row[end - 1].0 == ' ' {
                end -= 1;
            }

            let mut current_color: Option<termcolor::Color> = None;
            for &(ch, color) in &row[..end] {
                if color != current_color {
                    if current_color.is_some() {
                        out.push_str("\x1b[0m"); // Reset color
                    }
                    if let Some(c) = color {
                        out.push_str(&format!(
                            "\x1b[{}m",
                            Self::color_to_ansi(c)
                        ));
                    }
                    current_color = color;
                }
                out.push(ch);
            }
            if current_color.is_some() {
                out.push_str("\x1b[0m"); // Reset color at end of line
            }
            out.push('\n');
        }
        out
    }

    fn color_to_ansi(color: termcolor::Color) -> u8 {
        match color {
            Color::Black => 30,
            Color::Blue => 34,
            Color::Green => 32,
            Color::Red => 31,
            Color::Cyan => 36,
            Color::Magenta => 35,
            Color::Yellow => 33,
            Color::White => 37,
            _ => 37, // Default to white for other colors
        }
    }

    /// Returns whether this writer is configured for terminal output
    pub fn is_terminal(&self) -> bool {
        self.is_terminal
    }

    /// Returns whether this writer uses colors in terminal output
    pub fn uses_colors(&self) -> bool {
        self.use_colors
    }

    /// Set whether to use colors (only affects terminal output)
    pub fn set_use_colors(&mut self, use_colors: bool) {
        self.use_colors = use_colors && self.is_terminal;
    }

    fn ensure_size(&mut self, x: usize, y: usize) {
        if y >= self.height {
            let new_height = y + 1;
            let fill_width = self.width.max(1);
            self.grid
                .resize_with(new_height, || vec![(' ', None); fill_width]);
            self.height = new_height;
        }
        if x >= self.width {
            let new_width = x + 1;
            for row in &mut self.grid {
                row.resize(new_width, (' ', None));
            }
            self.width = new_width;
        }
    }

    fn to_ixy(&self, p: Point) -> (isize, isize) {
        (
            (p.x / self.scale).round() as isize,
            (p.y / self.scale).round() as isize,
        )
    }

    fn clamp_nonneg(ix: isize, iy: isize) -> Option<(usize, usize)> {
        if ix < 0 || iy < 0 {
            None
        } else {
            Some((ix as usize, iy as usize))
        }
    }

    fn set(&mut self, ix: isize, iy: isize, ch: char) {
        self.set_with_color(ix, iy, ch, None);
    }

    fn set_with_color(
        &mut self,
        ix: isize,
        iy: isize,
        ch: char,
        color: Option<termcolor::Color>,
    ) {
        if let Some((x, y)) = Self::clamp_nonneg(ix, iy) {
            self.ensure_size(x, y);
            self.grid[y][x] = (ch, color);
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

    fn get_line_char(&self, p0: Point, p1: Point) -> char {
        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;

        // Handle vertical and horizontal lines first
        if dx.abs() < 0.001 {
            return '|'; // Vertical line
        }
        if dy.abs() < 0.001 {
            return '-'; // Horizontal line
        }

        // Calculate slope
        let slope = dy / dx;
        let angle = slope.atan().to_degrees();

        // Choose character based on angle with simple distinctions
        let abs_angle = angle.abs();

        // Determine if we're going up-right or down-right based on dx and dy signs
        let going_down_right = (dx > 0.0 && dy > 0.0) || (dx < 0.0 && dy < 0.0);

        match abs_angle {
            a if a < 22.5 => '-', // Nearly horizontal
            a if a < 67.5 => {
                // Diagonal
                if going_down_right {
                    '\\'
                } else {
                    '/'
                }
            }
            _ => '|', // Nearly vertical
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

    fn rect_fill(
        &mut self,
        top_left: Point,
        size: Point,
        fill: char,
        color: Option<termcolor::Color>,
    ) {
        let (ix, iy) = self.to_ixy(top_left);
        let w = (size.x / self.scale).round().max(0.0) as isize;
        let h = (size.y / self.scale).round().max(0.0) as isize;
        for yy in 0..h {
            for xx in 0..w {
                self.set_with_color(ix + xx, iy + yy, fill, color);
            }
        }
    }

    fn rect_outline(&mut self, top_left: Point, size: Point) {
        let (ix, iy) = self.to_ixy(top_left);
        let w = (size.x / self.scale).round().max(0.0) as isize;
        let h = (size.y / self.scale).round().max(0.0) as isize;

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

    fn ellipse_outline(&mut self, center: Point, size: Point, _ch: char) {
        let a = (size.x / 2.0).max(0.0);
        let b = (size.y / 2.0).max(0.0);
        if a <= 0.0 || b <= 0.0 {
            return;
        }

        let (cx, cy) = self.to_ixy(center);
        let w = ((a * 2.0) / self.scale).round() as isize;
        let h = ((b * 2.0) / self.scale).round() as isize;

        // For very small circles, just use 'o'
        if w <= 2 || h <= 2 {
            self.set(cx, cy, 'o');
            return;
        }

        // Draw a more recognizable circle/ellipse using box drawing approach
        let left = cx - w / 2;
        let right = cx + w / 2;
        let top = cy - h / 2;
        let bottom = cy + h / 2;

        // Top and bottom horizontal lines
        for x in (left + 1)..right {
            self.set(x, top, '_');
            self.set(x, bottom, '_');
        }

        // Left and right vertical lines
        for y in (top + 1)..bottom {
            self.set(left, y, '|');
            self.set(right, y, '|');
        }

        // Corners - use diagonal characters for smoother appearance
        if w > 3 && h > 3 {
            self.set(left, top, '/');
            self.set(right, top, '\\');
            self.set(left, bottom, '\\');
            self.set(right, bottom, '/');
        } else {
            // For smaller circles, use corner characters
            self.set(left, top, '+');
            self.set(right, top, '+');
            self.set(left, bottom, '+');
            self.set(right, bottom, '+');
        }
    }

    fn ellipse_fill(
        &mut self,
        center: Point,
        size: Point,
        ch: char,
        color: Option<termcolor::Color>,
    ) {
        let a = (size.x / 2.0).max(0.0);
        let b = (size.y / 2.0).max(0.0);
        if a <= 0.0 || b <= 0.0 {
            return;
        }
        let cy0 = ((center.y - b) / self.scale).floor() as isize;
        let cy1 = ((center.y + b) / self.scale).ceil() as isize;

        for iy in cy0..=cy1 {
            // Compute span width using ellipse equation x = a * sqrt(1 - (y^2 / b^2))
            let yy = (iy as f64) * self.scale;
            let dy = yy - center.y;
            let inside = 1.0 - (dy * dy) / (b * b);
            if inside >= 0.0 {
                let span = a * inside.sqrt();
                let x0 = ((center.x - span) / self.scale).floor() as isize;
                let x1 = ((center.x + span) / self.scale).ceil() as isize;
                for ix in x0..=x1 {
                    self.set_with_color(ix, iy, ch, color);
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
        self.scale = look.font_size as f64;
        // Fill if requested (only in terminal mode), then outline.
        if look.fill_color.is_some() && self.is_terminal {
            let fill_color = if self.use_colors {
                Self::style_color_to_term_color(look.fill_color)
            } else {
                None
            };
            self.rect_fill(xy, size, '█', fill_color);
        }
        // Always draw outline for rectangles
        self.rect_outline(xy, size);
    }

    fn draw_line(
        &mut self,
        start: Point,
        stop: Point,
        look: &StyleAttr,
        _properties: Option<String>,
    ) {
        self.scale = look.font_size as f64;
        // Use slope-appropriate character based on line angle
        let line_char = self.get_line_char(start, stop);
        self.draw_line_segment(start, stop, line_char);
    }

    fn draw_circle(
        &mut self,
        xy: Point,
        size: Point,
        look: &StyleAttr,
        _properties: Option<String>,
    ) {
        self.scale = look.font_size as f64;
        // Fill then outline (only in terminal mode).
        if look.fill_color.is_some() && self.is_terminal {
            let fill_color = if self.use_colors {
                Self::style_color_to_term_color(look.fill_color)
            } else {
                None
            };
            self.ellipse_fill(xy, size, '●', fill_color);
        }
        // Always draw outline for circles
        self.ellipse_outline(xy, size, 'o');
    }

    fn draw_text(&mut self, xy: Point, text: &str, look: &StyleAttr) {
        self.scale = look.font_size as f64;
        self.text_at_center(xy, text);
    }

    fn draw_arrow(
        &mut self,
        path: &[(Point, Point)],
        dashed: bool,
        head: (bool, bool),
        look: &StyleAttr,
        _properties: Option<String>,
        text: &str,
    ) {
        self.scale = look.font_size as f64;
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

impl ASCIIWriter {
    fn style_color_to_term_color(
        color: Option<crate::core::color::Color>,
    ) -> Option<termcolor::Color> {
        color.and_then(|c| {
            let rgb = Self::extract_rgb_from_color(&c);
            // Simple color mapping - could be enhanced with better color matching
            if rgb.0 > 128 && rgb.1 < 128 && rgb.2 < 128 {
                Some(Color::Red)
            } else if rgb.0 < 128 && rgb.1 > 128 && rgb.2 < 128 {
                Some(Color::Green)
            } else if rgb.0 < 128 && rgb.1 < 128 && rgb.2 > 128 {
                Some(Color::Blue)
            } else if rgb.0 > 128 && rgb.1 > 128 && rgb.2 < 128 {
                Some(Color::Yellow)
            } else if rgb.0 > 128 && rgb.1 < 128 && rgb.2 > 128 {
                Some(Color::Magenta)
            } else if rgb.0 < 128 && rgb.1 > 128 && rgb.2 > 128 {
                Some(Color::Cyan)
            } else if rgb.0 > 200 && rgb.1 > 200 && rgb.2 > 200 {
                Some(Color::White)
            } else if rgb.0 < 100 && rgb.1 < 100 && rgb.2 < 100 {
                Some(Color::Black)
            } else {
                Some(Color::White) // Default
            }
        })
    }

    fn extract_rgb_from_color(
        color: &crate::core::color::Color,
    ) -> (u8, u8, u8) {
        // Extract RGB from the web color format
        let web_color = color.to_web_color();
        // Format is "#rrggbbaa", we want the first 6 characters after #
        let hex = &web_color[1..7];
        let color_val = u32::from_str_radix(hex, 16).unwrap_or(0);
        let r = ((color_val >> 16) & 0xFF) as u8;
        let g = ((color_val >> 8) & 0xFF) as u8;
        let b = (color_val & 0xFF) as u8;
        (r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::color::Color;
    use crate::core::style::StyleAttr;

    #[test]
    fn test_terminal_vs_non_terminal_fill() {
        // Test terminal mode - should fill rectangles
        let mut terminal_writer = ASCIIWriter::new_with_terminal_setting(true);
        let style = StyleAttr::new(
            Color::fast("black"),
            2,
            Some(Color::fast("red")),
            0,
            14,
        );

        terminal_writer.draw_rect(
            Point::new(0.0, 0.0),
            Point::new(56.0, 56.0),
            &style,
            None,
            None,
        );

        let terminal_output = terminal_writer.finalize();
        // Check for filled block character
        assert!(
            terminal_output.contains('█'),
            "Terminal output should contain fill characters: {}",
            terminal_output
        );

        // Test non-terminal mode - should not fill rectangles
        let mut non_terminal_writer =
            ASCIIWriter::new_with_terminal_setting(false);
        non_terminal_writer.draw_rect(
            Point::new(0.0, 0.0),
            Point::new(56.0, 56.0),
            &style,
            None,
            None,
        );

        let non_terminal_output = non_terminal_writer.finalize();
        assert!(
            !non_terminal_output.contains('█'),
            "Non-terminal output should not contain fill characters: {}",
            non_terminal_output
        );
        assert!(
            non_terminal_output.contains('+'),
            "Non-terminal output should still contain outline"
        );
    }

    #[test]
    fn test_color_settings() {
        let mut writer = ASCIIWriter::new_with_color_setting(true, false);
        assert!(writer.is_terminal());
        assert!(!writer.uses_colors());

        writer.set_use_colors(true);
        assert!(writer.uses_colors());

        // Non-terminal writer should not use colors even if requested
        let mut non_terminal_writer =
            ASCIIWriter::new_with_terminal_setting(false);
        non_terminal_writer.set_use_colors(true);
        assert!(!non_terminal_writer.uses_colors());
    }

    #[test]
    fn test_circle_terminal_behavior() {
        // Test terminal mode - should fill circles
        let mut terminal_writer = ASCIIWriter::new_with_terminal_setting(true);
        let style = StyleAttr::new(
            Color::fast("black"),
            2,
            Some(Color::fast("green")),
            0,
            14,
        );

        terminal_writer.draw_circle(
            Point::new(14.0, 14.0),
            Point::new(28.0, 28.0),
            &style,
            None,
        );

        let terminal_output = terminal_writer.finalize();
        assert!(
            terminal_output.contains('●'),
            "Terminal output should contain fill characters for circles"
        );

        // Test non-terminal mode - should not fill circles
        let mut non_terminal_writer =
            ASCIIWriter::new_with_terminal_setting(false);
        non_terminal_writer.draw_circle(
            Point::new(14.0, 14.0),
            Point::new(28.0, 28.0),
            &style,
            None,
        );

        let non_terminal_output = non_terminal_writer.finalize();
        assert!(!non_terminal_output.contains('●'),
                "Non-terminal output should not contain fill characters for circles");
        assert!(
            non_terminal_output.contains('o'),
            "Non-terminal output should still contain circle outline"
        );
    }

    #[test]
    fn test_terminal_color_output() {
        // Test that terminal mode with colors enabled produces ANSI escape codes
        let mut terminal_writer =
            ASCIIWriter::new_with_color_setting(true, true);
        let style = StyleAttr::new(
            Color::fast("black"),
            2,
            Some(Color::fast("red")),
            0,
            14,
        );

        terminal_writer.draw_rect(
            Point::new(0.0, 0.0),
            Point::new(56.0, 56.0),
            &style,
            None,
            None,
        );

        let colored_output = terminal_writer.finalize();
        assert!(
            colored_output.contains("\x1b[31m"), // ANSI red color code
            "Terminal color output should contain ANSI escape codes: {}",
            colored_output
        );
        assert!(
            colored_output.contains("\x1b[0m"), // ANSI reset code
            "Terminal color output should contain ANSI reset codes: {}",
            colored_output
        );

        // Test that terminal mode without colors doesn't produce ANSI codes
        let mut no_color_writer =
            ASCIIWriter::new_with_color_setting(true, false);
        no_color_writer.draw_rect(
            Point::new(0.0, 0.0),
            Point::new(56.0, 56.0),
            &style,
            None,
            None,
        );

        let plain_output = no_color_writer.finalize();
        assert!(
            !plain_output.contains("\x1b["),
            "Non-color terminal output should not contain ANSI codes: {}",
            plain_output
        );
    }
}
