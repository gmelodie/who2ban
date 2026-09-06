//! A picture of the screen, for reading the draft off.
//!
//! The grab is taken from the root window at screen coordinates rather than from the
//! game's own window. Asking X for a window's contents can hand back a pixmap the
//! server kept rather than what is on the glass, which is silently stale and, for a
//! window that is not currently mapped, entirely wrong.

/// Straight RGB, three bytes to the pixel, top row first.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

impl Frame {
    /// A grab that arrived as one flat colour is a dropped frame, not a picture. The
    /// game never draws a uniform screen, so this costs nothing and catches the case
    /// that would otherwise be read as an empty draft.
    pub fn looks_drawn(&self) -> bool {
        let (mut lo, mut hi) = (255u8, 0u8);
        for chunk in self.rgb.chunks(3 * 97) {
            if let Some(&v) = chunk.first() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        hi.saturating_sub(lo) > 12
    }
}

/// Where a window is on the desktop, in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

#[derive(Debug)]
pub enum Error {
    Unsupported,
    Display(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unsupported => write!(f, "no screen grabber on this platform"),
            Error::Display(e) => write!(f, "display: {e}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

/// The screens this machine is drawing on.
#[cfg(all(unix, not(target_os = "macos")))]
pub use x11::{Screen, find_window, screens};

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{Screen, find_window, screens};

/// Nothing to grab with, which is macOS and anything else. The draft reader stays quiet
/// and the client goes on working from the battlelobby the way it always has.
#[cfg(not(any(all(unix, not(target_os = "macos")), windows)))]
mod nothing {
    use super::{Error, Frame, Rect};

    pub struct Screen {
        pub x: i16,
        pub y: i16,
        pub w: u16,
        pub h: u16,
    }

    impl Screen {
        pub fn grab(&self) -> Result<Frame, Error> {
            Err(Error::Unsupported)
        }

        pub fn grab_region(&self, _x: i16, _y: i16, _w: u16, _h: u16) -> Result<Frame, Error> {
            Err(Error::Unsupported)
        }
    }

    pub fn screens() -> Result<Vec<Screen>, Error> {
        Err(Error::Unsupported)
    }

    pub fn find_window(_title: &str) -> Result<Option<Rect>, Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(not(any(all(unix, not(target_os = "macos")), windows)))]
pub use nothing::{Screen, find_window, screens};
