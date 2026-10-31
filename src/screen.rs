//! Her eyes: GDI captures of the primary monitor. Nothing is written to disk; a capture lives
//! only long enough to be described by the model.

use base64::Engine as _;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SetWindowDisplayAffinity, SM_CXSCREEN, SM_CYSCREEN};

/// Windows 10 2004+: the window is left out of every screen capture (hers included).
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;

/// Side of the change-detection thumbnail grid.
const THUMB_W: usize = 64;
const THUMB_H: usize = 36;

pub struct Shot {
    pub w: u32,
    pub h: u32,
    /// Top-down BGRA.
    bgra: Vec<u8>,
}

pub fn exclude_from_capture(hwnd: isize) {
    set_capturable(hwnd, false);
}

/// Whether a window appears in screen captures (hers and everyone else's).
pub fn set_capturable(hwnd: isize, capturable: bool) {
    const WDA_NONE: u32 = 0;
    unsafe {
        SetWindowDisplayAffinity(hwnd as HWND, if capturable { WDA_NONE } else { WDA_EXCLUDEFROMCAPTURE });
    }
}

/// Flat grey: what she sees where her own chat window is, when it cannot be left out.
const MASK: [u8; 4] = [58, 58, 58, 255];

pub fn capture() -> Option<Shot> {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp);
        let ok = BitBlt(dc, 0, 0, w, h, screen, 0, 0, SRCCOPY | CAPTUREBLT) != 0;
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(dc, bmp, 0, h as u32, bgra.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        (ok && lines == h).then_some(Shot { w: w as u32, h: h as u32, bgra })
    }
}

impl Shot {
    /// Paints a rectangle (screen pixels; whatever falls outside is ignored) flat grey.
    pub fn mask(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let (x1, y1) = ((x + w).clamp(0, self.w as i32) as usize, (y + h).clamp(0, self.h as i32) as usize);
        for row in y0..y1 {
            let start = (row * self.w as usize + x0) * 4;
            for px in self.bgra[start..start + (x1.saturating_sub(x0)) * 4].chunks_exact_mut(4) {
                px.copy_from_slice(&MASK);
            }
        }
    }

    /// A tiny grayscale grid to tell whether the screen changed, without the model.
    pub fn thumb(&self) -> Vec<u8> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut out = vec![0u8; THUMB_W * THUMB_H];
        for ty in 0..THUMB_H {
            for tx in 0..THUMB_W {
                let (x0, x1) = (tx * w / THUMB_W, ((tx + 1) * w / THUMB_W).max(tx * w / THUMB_W + 1));
                let (y0, y1) = (ty * h / THUMB_H, ((ty + 1) * h / THUMB_H).max(ty * h / THUMB_H + 1));
                let mut sum = 0u64;
                let mut n = 0u64;
                for y in (y0..y1).step_by(4) {
                    for x in (x0..x1).step_by(4) {
                        let i = (y * w + x) * 4;
                        let p = &self.bgra[i..i + 3];
                        sum += (p[0] as u64 * 29 + p[1] as u64 * 150 + p[2] as u64 * 77) >> 8;
                        n += 1;
                    }
                }
                out[ty * THUMB_W + tx] = (sum / n.max(1)) as u8;
            }
        }
        out
    }

    /// The screen as a JPEG (base64) at most `max_w` pixels wide, for the vision encoder.
    pub fn jpeg_b64(&self, max_w: u32) -> Option<String> {
        let mut rgb = Vec::with_capacity((self.w * self.h * 3) as usize);
        for p in self.bgra.chunks_exact(4) {
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        let img = image::RgbImage::from_raw(self.w, self.h, rgb)?;
        let img = if self.w > max_w {
            let nh = (self.h as u64 * max_w as u64 / self.w as u64) as u32;
            image::imageops::resize(&img, max_w, nh, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 78).encode_image(&img).ok()?;
        Some(base64::engine::general_purpose::STANDARD.encode(out))
    }
}

/// How different two thumbnails are, in [0, 1]: the share of cells whose brightness moved
/// visibly. New lines of text or a scroll change many cells a little, which a mean would miss.
pub fn change(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    let moved = a.iter().zip(b).filter(|(x, y)| (**x as i32 - **y as i32).abs() > 4).count();
    moved as f32 / a.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_masked_window_does_not_count_as_a_change() {
        let mut a = Shot { w: 640, h: 360, bgra: vec![200; 640 * 360 * 4] };
        let mut b = Shot { w: 640, h: 360, bgra: vec![200; 640 * 360 * 4] };
        // The chat window got new lines of text in b; both are masked where it is.
        for p in b.bgra[(100 * 640 + 400) * 4..(100 * 640 + 600) * 4].iter_mut() {
            *p = 0;
        }
        a.mask(380, 50, 300, 400);
        b.mask(380, 50, 300, 400);
        assert_eq!(change(&a.thumb(), &b.thumb()), 0.0);
        assert_eq!(&b.bgra[(100 * 640 + 500) * 4..(100 * 640 + 500) * 4 + 4], &MASK);
        assert_eq!(b.bgra[(100 * 640 + 10) * 4], 200, "the rest is untouched");
        a.mask(-50, -50, 10, 10); // fully outside: nothing
    }

    #[test]
    fn change_is_zero_for_the_same_screen() {
        let a = vec![10u8; THUMB_W * THUMB_H];
        let mut b = a.clone();
        assert_eq!(change(&a, &b), 0.0);
        b[0] = 12;
        assert_eq!(change(&a, &b), 0.0, "noise is not a change");
        for c in b.iter_mut().take(THUMB_W * 2) {
            *c = 60;
        }
        assert!(change(&a, &b) > 0.05, "two rows of new content are");
        assert_eq!(change(&a, &[]), 1.0);
    }
}
