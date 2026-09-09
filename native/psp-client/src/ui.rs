use alloc::{string::String, vec, vec::Vec};

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 272;
/// PSP Psm8888: 0xAABBGGRR, RGBA bytes in little-endian memory.
pub type Color = u32;
pub const BG: Color = 0xff1f1710;
pub const PANEL: Color = 0xff38291c;
pub const TEXT: Color = 0xfffaf5f3;
pub const MUTED: Color = 0xffc9b9ad;
pub const ACCENT: Color = 0xffffcba3;
pub const ERROR: Color = 0xff9393ff;
const LINE: Color = 0xff5a4736;
const CHROME: Color = 0xff0f0b08;
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

fn state_color(state: &str) -> Color {
    if state.contains("Chyba") {
        ERROR
    } else if state.contains("Čeká") {
        0xff85cff4
    } else if state.contains("Běží") {
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
        self.small(10, 3, "T3 PSP", TEXT);
        let connection = ellipsis(connection, 330);
        self.small(
            470 - width(SMALL_FONT, &connection) as i32,
            3,
            &connection,
            MUTED,
        );
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
        assert_ne!(state_color("Čeká"), state_color("Běží"));
        assert_eq!(state_color("Chyba"), ERROR);
        assert_eq!(state_color("Nečinný"), MUTED);
        assert_eq!(state_color("1 / 12"), MUTED);
    }
}
