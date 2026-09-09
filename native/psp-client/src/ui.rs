use alloc::{string::String, vec, vec::Vec};

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 272;
/// PSP Psm8888: 0xAABBGGRR, RGBA bytes in little-endian memory.
pub type Color = u32;
pub const fn rgb(value: u32) -> Color {
    0xff000000 | (value & 255) << 16 | (value & 0xff00) | value >> 16
}
pub const BG: Color = rgb(0x0a0a0a);
pub const PANEL: Color = rgb(0x111111);
pub const SELECTED: Color = rgb(0x1c1c1c);
pub const BUBBLE: Color = rgb(0x171717);
pub const TEXT: Color = rgb(0xf5f5f5);
pub const MUTED: Color = rgb(0xa3a3a3);
pub const ACCENT: Color = rgb(0x60a5fa);
pub const ERROR: Color = rgb(0xf87171);
pub const AMBER: Color = rgb(0xfbbf24);
pub const LINE: Color = rgb(0x262626);
pub const CHROME: Color = rgb(0x000000);
const BODY_FONT: &[u8] = include_bytes!("../assets/font14.bin");
const SMALL_FONT: &[u8] = include_bytes!("../assets/font12.bin");
const RECORD: usize = 329;

fn glyph(font: &[u8], ch: char) -> &[u8] {
    // The common Latin range is directly indexed; only symbols need a scan.
    let code = ch as usize;
    if (32..383).contains(&code) {
        return &font[(code - 32) * RECORD..(code - 31) * RECORD];
    }
    for item in font[351 * RECORD..].chunks_exact(RECORD) {
        if u32::from_le_bytes([item[0], item[1], item[2], item[3]]) == ch as u32 {
            return item;
        }
    }
    &font[31 * RECORD..32 * RECORD]
}

fn width(font: &[u8], text: &str) -> usize {
    text.chars().map(|ch| glyph(font, ch)[4] as usize).sum()
}

pub fn text_width(text: &str) -> usize {
    width(BODY_FONT, text)
}

pub fn small_width(text: &str) -> usize {
    width(SMALL_FONT, text)
}

pub fn state_color(state: &str) -> Color {
    if state.contains("Error") {
        ERROR
    } else if state.contains("Waiting") {
        AMBER
    } else if state.contains("Working") {
        ACCENT
    } else {
        MUTED
    }
}

pub fn ellipsis(text: &str, available: usize) -> String {
    let text: String = text
        .chars()
        .map(|ch| {
            if ch.is_whitespace() || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect();
    if text_width(&text) <= available {
        return text;
    }
    let tail = text_width("…");
    if tail > available {
        return String::new();
    }
    let mut result = String::new();
    let mut used = tail;
    for ch in text.chars() {
        let advance = glyph(BODY_FONT, ch)[4] as usize;
        if used + advance > available {
            break;
        }
        result.push(ch);
        used += advance;
    }
    result.push('…');
    result
}

pub fn wrap_text(text: &str, available: usize) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty()
                && text_width(&line) + text_width(" ") + text_width(word) > available
            {
                result.push(core::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            for ch in word.chars() {
                if !line.is_empty()
                    && text_width(&line) + glyph(BODY_FONT, ch)[4] as usize > available
                {
                    result.push(core::mem::take(&mut line));
                }
                line.push(ch);
            }
        }
        result.push(line);
    }
    result
}

pub struct Frame {
    pub pixels: Vec<u32>,
}

impl Frame {
    pub fn new() -> Self {
        Self {
            pixels: vec![BG; WIDTH * HEIGHT],
        }
    }
    pub fn clear(&mut self) {
        self.pixels.fill(BG);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        let left = x.clamp(0, WIDTH as i32) as usize;
        let right = x.saturating_add(w).clamp(0, WIDTH as i32) as usize;
        let top = y.clamp(0, HEIGHT as i32) as usize;
        let bottom = y.saturating_add(h).clamp(0, HEIGHT as i32) as usize;
        for row in top..bottom {
            self.pixels[row * WIDTH + left..row * WIDTH + right].fill(color);
        }
    }

    // Integer geometry keeps this usable on the PSP without floating-point helpers.
    pub fn rounded(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Color) {
        let r = radius.max(0).min(w / 2).min(h / 2);
        for row in 0..h {
            let dy = if row < r {
                r - row - 1
            } else if row >= h - r {
                row - (h - r)
            } else {
                0
            };
            let mut dx = r;
            while dx * dx + dy * dy > r * r {
                dx -= 1;
            }
            let inset = r - dx;
            self.rect(x + inset, y + row, w - 2 * inset, 1, color);
        }
    }
    pub fn outline(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        border: Color,
        fill: Color,
    ) {
        self.rounded(x, y, w, h, radius, border);
        self.rounded(x + 1, y + 1, w - 2, h - 2, radius - 1, fill);
    }
    pub fn compact_footer(&mut self, text: &str) {
        self.rect(0, 248, 480, 24, CHROME);
        self.rect(0, 248, 480, 1, LINE);
        self.small(10, 252, text, MUTED);
    }
    pub fn connection(&mut self, end: i32, y: i32, connection: &str) {
        if let Some((label, battery)) = connection.rsplit_once("  •  ") {
            let battery = ellipsis(battery, 55);
            let bx = end - small_width(&battery) as i32;
            self.small(bx, y, &battery, MUTED);
            let icon = bx - 24;
            self.outline(icon, y + 5, 15, 9, 2, MUTED, BG);
            self.rect(icon + 15, y + 7, 2, 5, MUTED);
            if let Ok(percent) = battery.trim_end_matches('%').trim().parse::<i32>() {
                self.rect(icon + 2, y + 7, 11 * percent.clamp(0, 100) / 100, 5, MUTED);
            }
            let label = ellipsis(label, 220);
            self.small(icon - 6 - small_width(&label) as i32, y, &label, MUTED);
        } else {
            let text = ellipsis(connection, 300);
            self.small(end - small_width(&text) as i32, y, &text, MUTED);
        }
    }

    fn draw_text(&mut self, mut x: i32, mut y: i32, text: &str, color: Color, font: &[u8]) {
        let origin = x;
        for ch in text.chars() {
            if ch == '\n' {
                x = origin;
                y += 18;
                continue;
            }
            let item = glyph(font, ch);
            for row in 0..18 {
                let py = y + row;
                if !(0..HEIGHT as i32).contains(&py) {
                    continue;
                }
                for col in 0..18 {
                    let px = x + col;
                    if !(0..WIDTH as i32).contains(&px) {
                        continue;
                    }
                    let alpha = item[5 + (row * 18 + col) as usize] as u32;
                    if alpha == 0 {
                        continue;
                    }
                    let target = &mut self.pixels[py as usize * WIDTH + px as usize];
                    let mut blended = 0xff000000;
                    for shift in [0, 8, 16] {
                        let fg = (color >> shift) & 255;
                        let bg = (*target >> shift) & 255;
                        blended |= ((fg * alpha + bg * (255 - alpha) + 127) / 255) << shift;
                    }
                    *target = blended;
                }
            }
            x += item[4] as i32;
        }
    }

    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Color) {
        self.draw_text(x, y, text, color, BODY_FONT);
    }
    pub fn small(&mut self, x: i32, y: i32, text: &str, color: Color) {
        self.draw_text(x, y, text, color, SMALL_FONT);
    }
    pub fn header(&mut self, title: &str, state: &str, connection: &str) {
        self.rect(0, 0, 480, 23, CHROME);
        self.small(10, 3, "T3 Code", TEXT);
        self.connection(470, 3, connection);
        self.rect(0, 23, 480, 33, BG);
        let state = ellipsis(state, 220);
        let state_width = width(SMALL_FONT, &state);
        self.text(
            10,
            30,
            &ellipsis(title, 450usize.saturating_sub(state_width + 12)),
            TEXT,
        );
        self.small(469 - state_width as i32, 32, &state, state_color(&state));
        self.rect(0, 55, 480, 1, LINE);
    }
    pub fn footer(&mut self, first: &str, second: &str) {
        self.rect(0, 232, 480, 40, CHROME);
        self.rect(0, 232, 480, 1, LINE);
        self.small(10, 235, first, TEXT);
        self.small(10, 252, second, MUTED);
    }
    pub fn scrollbar(&mut self, start: usize, total: usize, visible: usize) {
        if total <= visible || visible == 0 {
            return;
        }
        let track = 164usize;
        let height = (track * visible / total).max(8).min(track);
        let offset = (track - height) * start.min(total - visible) / (total - visible);
        self.rect(474, 62, 3, track as i32, LINE);
        self.rect(474, 62 + offset as i32, 3, height as i32, ACCENT);
    }

    #[cfg(target_os = "psp")]
    pub fn present(&self) {
        use psp::sys::*;
        // Two 512x272 RGBA surfaces occupy 1,114,112 bytes of the 2 MiB VRAM.
        // Only the UI thread presents. Do not interleave psp::dprintln! or GPU use.
        static mut BACK_BUFFER: usize = 1;
        static mut INITIALIZED: bool = false;
        unsafe {
            if !INITIALIZED {
                let result = sceDisplaySetMode(DisplayMode::Lcd, WIDTH, HEIGHT);
                if result != 0 {
                    crate::diagnostics::display_error("set mode", result);
                }
            }
            let base = (sceGeEdramGetAddr() as usize | 0x40000000) as *mut u32;
            let output = base.add(BACK_BUFFER * 512 * HEIGHT);
            for row in 0..HEIGHT {
                core::ptr::copy_nonoverlapping(
                    self.pixels.as_ptr().add(row * WIDTH),
                    output.add(row * 512),
                    WIDTH,
                );
            }
            // NEXTFRAME establishes the pixel format/stride even when the
            // previous program used a different layout. IMMEDIATE can reject it.
            let result = sceDisplaySetFrameBuf(
                output.cast(),
                512,
                DisplayPixelFormat::Psm8888,
                DisplaySetBufSync::NextFrame,
            );
            if result != 0 {
                crate::diagnostics::display_error("set framebuffer", result);
            }
            let result = sceDisplayWaitVblankStart();
            if result < 0 {
                crate::diagnostics::display_error("wait vblank", result as u32);
            }
            INITIALIZED = true;
            BACK_BUFFER ^= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_czech_text_wraps_without_losing_words() {
        assert!(text_width("iii") < text_width("WWW"));
        let original = "Přílišžluťoučkýkůň úpěl ďábelské ódy";
        let lines = wrap_text(original, 65);
        assert!(lines.iter().all(|line| text_width(line) <= 65));
        assert_eq!(lines.concat().replace(' ', ""), original.replace(' ', ""));
        assert_eq!(wrap_text("a\n\nb", 100), vec!["a", "", "b"]);
    }

    #[test]
    fn ellipsis_fits_available_pixels() {
        let title = "Hlasové ovládání PSP";
        for available in 0..200 {
            assert!(text_width(&ellipsis(title, available)) <= available);
        }
        assert_eq!(ellipsis(title, 400), title);
    }

    #[test]
    fn ellipsis_keeps_untrusted_titles_on_one_line() {
        let title = "Title\nline\tend\r";
        assert_eq!(ellipsis(title, 400), "Title line end ");
        for available in 0..200 {
            let result = ellipsis(title, available);
            assert!(text_width(&result) <= available);
            assert!(!result.chars().any(char::is_control));
        }
        assert_eq!(ellipsis("A\u{2028}B\0C", 400), "A B C");
    }

    #[test]
    fn offscreen_primitives_are_clipped() {
        let mut frame = Frame::new();
        frame.rect(-100, -100, 30, 30, ERROR);
        frame.text(-100, -100, "Český text", TEXT);
        assert!(frame.pixels.iter().all(|&pixel| pixel == BG));
        frame.rect(-2, -2, 4, 4, ACCENT);
        assert_eq!(
            frame
                .pixels
                .iter()
                .filter(|&&pixel| pixel == ACCENT)
                .count(),
            4
        );
        frame.text(479, 271, "Příliš žluťoučký kůň", TEXT);
        assert_eq!(frame.pixels.len(), WIDTH * HEIGHT);
    }

    #[test]
    fn state_colors_distinguish_attention_from_running() {
        assert_ne!(state_color("Waiting"), state_color("Working"));
        assert_eq!(state_color("Error"), ERROR);
        assert_eq!(state_color("Idle"), MUTED);
        assert_eq!(state_color("1 / 12"), MUTED);
    }
}
