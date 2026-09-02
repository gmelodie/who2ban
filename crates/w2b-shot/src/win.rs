//! Windows, through GDI.
//!
//! The picture is taken from the screen's own device context rather than the game
//! window's, for the same reason the X11 side reads the root: asking a window to draw
//! itself returns what it would draw, not what is on the glass, and for a window that is
//! occluded or minimised that is not the same thing.
//!
//! A client running truly exclusive fullscreen may hand back a blank picture, which
//! `Frame::looks_drawn` catches and reports as no draft rather than an empty one.
//! Borderless windowed, which is what the game defaults to, copies fine.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetSystemMetrics, GetWindowRect, GetWindowTextW, IsWindowVisible,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::{Error, Frame, Rect};

/// The whole desktop. Several monitors make one virtual screen, so a window is found and
/// grabbed in the same coordinates whichever monitor the game is on.
pub struct Screen {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

pub fn screens() -> Result<Vec<Screen>, Error> {
    // SAFETY: reading four documented metrics, no pointers involved.
    let (x, y, w, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if w <= 0 || h <= 0 {
        return Err(Error::Display("no virtual screen".into()));
    }
    Ok(vec![Screen {
        x: clamp16(x),
        y: clamp16(y),
        w: w.clamp(0, u16::MAX as i32) as u16,
        h: h.clamp(0, u16::MAX as i32) as u16,
    }])
}

impl Screen {
    pub fn grab(&self) -> Result<Frame, Error> {
        self.grab_region(self.x, self.y, self.w, self.h)
    }

    pub fn grab_region(&self, x: i16, y: i16, w: u16, h: u16) -> Result<Frame, Error> {
        if w == 0 || h == 0 {
            return Err(Error::Display("empty region".into()));
        }
        // SAFETY: every handle below is released on the way out, including on the error
        // paths, and the bitmap's pixels are read only while its DIB section is alive.
        unsafe { copy_from_screen(x as i32, y as i32, w as usize, h as usize) }
    }
}

unsafe fn copy_from_screen(x: i32, y: i32, w: usize, h: usize) -> Result<Frame, Error> {
    let fail = |what: &str| Error::Display(what.to_string());

    let screen_dc = unsafe { GetDC(std::ptr::null_mut()) };
    if screen_dc.is_null() {
        return Err(fail("no screen device context"));
    }
    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_null() {
        unsafe { ReleaseDC(std::ptr::null_mut(), screen_dc) };
        return Err(fail("no memory device context"));
    }

    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        // Negative, so the rows arrive top down and match every other picture here.
        biHeight: -(h as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB as u32,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    let mut pixels: *mut c_void = std::ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            memory_dc,
            &info,
            DIB_RGB_COLORS,
            &mut pixels,
            std::ptr::null_mut(),
            0,
        )
    };
    if bitmap.is_null() || pixels.is_null() {
        unsafe {
            DeleteDC(memory_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
        }
        return Err(fail("no bitmap"));
    }

    let previous = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    // CAPTUREBLT so layered windows on top of the game are copied as they appear.
    let copied = unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            w as i32,
            h as i32,
            screen_dc,
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        )
    };

    let frame = if copied != 0 {
        let bgra = unsafe { std::slice::from_raw_parts(pixels as *const u8, w * h * 4) };
        let mut rgb = Vec::with_capacity(w * h * 3);
        for px in bgra.chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        Ok(Frame { w, h, rgb })
    } else {
        Err(fail("the screen would not copy"))
    };

    unsafe {
        SelectObject(memory_dc, previous);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);
    }
    frame
}

/// What `find_window` is looking for and the best it has found, handed to the callback.
struct Hunt {
    title: Vec<u16>,
    best: Option<Rect>,
}

/// The game's window, wherever the desktop put it.
pub fn find_window(title: &str) -> Result<Option<Rect>, Error> {
    let mut hunt = Hunt {
        title: title.encode_utf16().collect(),
        best: None,
    };
    // SAFETY: the pointer handed over lives for the whole call and is only read back
    // inside the callback, which cannot outlive it.
    unsafe {
        EnumWindows(Some(consider), &mut hunt as *mut Hunt as LPARAM);
    }
    Ok(hunt.best)
}

unsafe extern "system" fn consider(window: HWND, carried: LPARAM) -> i32 {
    // SAFETY: the pointer is the `Hunt` that `find_window` is still borrowing.
    let hunt = unsafe { &mut *(carried as *mut Hunt) };
    const KEEP_LOOKING: i32 = 1;

    if unsafe { IsWindowVisible(window) } == 0 {
        return KEEP_LOOKING;
    }
    let mut text = [0u16; 256];
    let written = unsafe { GetWindowTextW(window, text.as_mut_ptr(), text.len() as i32) };
    if written <= 0 || text[..written as usize] != hunt.title[..] {
        return KEEP_LOOKING;
    }

    let mut area = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(window, &mut area) } == 0 {
        return KEEP_LOOKING;
    }
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    if w < 200 || h < 200 {
        return KEEP_LOOKING;
    }
    // The client keeps more than one window under the same name; the drawn one is the
    // biggest.
    let bigger = hunt
        .best
        .map(|b| w * h > i32::from(b.w) * i32::from(b.h))
        .unwrap_or(true);
    if bigger {
        hunt.best = Some(Rect {
            x: clamp16(area.left),
            y: clamp16(area.top),
            w: w.clamp(0, u16::MAX as i32) as u16,
            h: h.clamp(0, u16::MAX as i32) as u16,
        });
    }
    KEEP_LOOKING
}

/// A monitor to the left of the primary one has a negative origin, and a desktop wider
/// than a signed short does not exist.
fn clamp16(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}
