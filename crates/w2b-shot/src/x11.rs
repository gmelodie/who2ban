//! X11. Wayland refuses this outright, and a session running under it will find no
//! screens here, which is the honest answer rather than a black picture.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, Window};
use x11rb::rust_connection::RustConnection;

use crate::{Error, Frame, Rect};

/// One monitor, as X lays it out on the root window.
///
/// The connection is held open. Grabbing happens every couple of seconds for as long as
/// the program runs, and standing up a fresh connection to the display each time is a
/// handshake nobody needs.
pub struct Screen {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    root: Window,
    conn: RustConnection,
}

pub fn screens() -> Result<Vec<Screen>, Error> {
    let (conn, num) = x11rb::connect(None).map_err(|e| Error::Display(e.to_string()))?;
    let setup = conn.setup().roots[num].clone();

    // The root spans every monitor. Reading a name off the draft does not care which
    // monitor the game is on, so the whole desktop is one picture and the geometry is
    // measured within it.
    Ok(vec![Screen {
        x: 0,
        y: 0,
        w: setup.width_in_pixels,
        h: setup.height_in_pixels,
        root: setup.root,
        conn,
    }])
}

impl Screen {
    pub fn grab(&self) -> Result<Frame, Error> {
        self.grab_region(self.x, self.y, self.w, self.h)
    }

    pub fn grab_region(&self, x: i16, y: i16, w: u16, h: u16) -> Result<Frame, Error> {
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, x, y, w, h, !0)
            .map_err(|e| Error::Display(e.to_string()))?
            .reply()
            .map_err(|e| Error::Display(e.to_string()))?;

        let (w, h) = (w as usize, h as usize);
        let stride = reply.data.len().checked_div(h).unwrap_or(0);
        let bytes = stride.checked_div(w).unwrap_or(0);
        if bytes < 3 {
            return Err(Error::Display(format!("{bytes} bytes per pixel")));
        }

        // X hands back the native order, which on every machine this runs on is BGRX.
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in 0..h {
            let line = &reply.data[row * stride..row * stride + w * bytes];
            for px in line.chunks_exact(bytes) {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        Ok(Frame { w, h, rgb })
    }
}

/// The game's window, wherever the desktop put it. Everything the draft reader measures
/// is a fraction of this rather than of the screen: the game may be windowed, may be on
/// the second monitor, and on this machine is both.
pub fn find_window(title: &str) -> Result<Option<Rect>, Error> {
    let (conn, num) = x11rb::connect(None).map_err(|e| Error::Display(e.to_string()))?;
    let root = conn.setup().roots[num].root;
    let names = Atoms::intern(&conn)?;
    let mut best: Option<Rect> = None;
    walk(&conn, root, title, &names, &mut best)?;
    Ok(best)
}

/// Interned once and carried down the tree: asking the server to name these properties
/// again for every window turns one lookup into hundreds of round trips.
struct Atoms {
    utf8: u32,
    net_name: u32,
}

impl Atoms {
    fn intern<C: Connection>(conn: &C) -> Result<Atoms, Error> {
        let ask = |name: &[u8]| -> u32 {
            conn.intern_atom(true, name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map_or(0, |r| r.atom)
        };
        Ok(Atoms {
            utf8: ask(b"UTF8_STRING"),
            net_name: ask(b"_NET_WM_NAME"),
        })
    }
}

fn walk<C: Connection>(
    conn: &C,
    window: Window,
    title: &str,
    names: &Atoms,
    best: &mut Option<Rect>,
) -> Result<(), Error> {
    let tree = conn
        .query_tree(window)
        .map_err(|e| Error::Display(e.to_string()))?
        .reply()
        .map_err(|e| Error::Display(e.to_string()))?;

    for child in tree.children {
        if names_match(conn, child, title, names) {
            if let Ok(geom) = conn
                .get_geometry(child)
                .map_err(|e| Error::Display(e.to_string()))?
                .reply()
            {
                // Coordinates are relative to the parent, so ask where that really is.
                let here = conn
                    .translate_coordinates(child, window, 0, 0)
                    .map_err(|e| Error::Display(e.to_string()))?
                    .reply()
                    .map(|t| (t.dst_x, t.dst_y))
                    .unwrap_or((geom.x, geom.y));
                let found = Rect {
                    x: here.0,
                    y: here.1,
                    w: geom.width,
                    h: geom.height,
                };
                // The game reparents, so several windows carry the name; the drawn one
                // is the biggest.
                let bigger = best
                    .map(|b| u32::from(found.w) * u32::from(found.h) > u32::from(b.w) * u32::from(b.h))
                    .unwrap_or(true);
                if bigger && found.w > 200 && found.h > 200 {
                    *best = Some(found);
                }
            }
        }
        walk(conn, child, title, names, best)?;
    }
    Ok(())
}

fn names_match<C: Connection>(conn: &C, window: Window, title: &str, names: &Atoms) -> bool {
    title_of(conn, window, names).is_some_and(|n| n == title)
}

/// A window may publish its title as either property, and the modern one wins when both
/// are set. Reading only `WM_NAME` finds nothing on most of what runs today.
fn title_of<C: Connection>(conn: &C, window: Window, names: &Atoms) -> Option<String> {
    for (property, kind) in [
        (names.net_name, names.utf8),
        (u32::from(x11rb::protocol::xproto::AtomEnum::WM_NAME), u32::from(x11rb::protocol::xproto::AtomEnum::STRING)),
    ] {
        if property == 0 {
            continue;
        }
        let Ok(cookie) = conn.get_property(false, window, property, kind, 0, 1024) else {
            continue;
        };
        let Ok(reply) = cookie.reply() else { continue };
        if reply.value.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(&reply.value).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}
