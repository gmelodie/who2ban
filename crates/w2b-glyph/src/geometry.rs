//! Where on the draft screen the ten names are written.
//!
//! Measured off one 3840 by 2160 capture of a Storm League draft and held as fractions
//! of the game's window, so the same numbers serve a smaller screen and a windowed
//! client. They have only ever been checked against that one capture and that one
//! aspect ratio; a 21:9 screen lays the draft out differently and will want its own.

/// A seat on the draft screen. The rows run down the panel in lobby order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat {
    /// Which panel: the two teams face each other across the screen.
    pub right_hand: bool,
    pub row: u8,
}

/// A box as a share of the window, left, top, right, bottom.
#[derive(Debug, Clone, Copy)]
pub struct Share {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// The name banners, generously drawn: a box that clips a name loses the letters at its
/// end, and a box with scenery in it loses nothing, because what is not on the baseline
/// is discarded anyway.
pub const BANNERS: [(Seat, Share); 10] = [
    (Seat { right_hand: false, row: 0 }, Share { x0: 0.0000, y0: 0.1481, x1: 0.1146, y1: 0.2477 }),
    (Seat { right_hand: false, row: 1 }, Share { x0: 0.0365, y0: 0.3056, x1: 0.1719, y1: 0.4074 }),
    (Seat { right_hand: false, row: 2 }, Share { x0: 0.0000, y0: 0.4676, x1: 0.1354, y1: 0.5810 }),
    (Seat { right_hand: false, row: 3 }, Share { x0: 0.0365, y0: 0.6111, x1: 0.1771, y1: 0.7199 }),
    (Seat { right_hand: false, row: 4 }, Share { x0: 0.0000, y0: 0.7685, x1: 0.1172, y1: 0.8681 }),
    (Seat { right_hand: true,  row: 0 }, Share { x0: 0.8958, y0: 0.1481, x1: 1.0000, y1: 0.2477 }),
    (Seat { right_hand: true,  row: 1 }, Share { x0: 0.8516, y0: 0.3056, x1: 0.9870, y1: 0.4074 }),
    (Seat { right_hand: true,  row: 2 }, Share { x0: 0.8802, y0: 0.4676, x1: 1.0000, y1: 0.5810 }),
    (Seat { right_hand: true,  row: 3 }, Share { x0: 0.8385, y0: 0.6111, x1: 0.9896, y1: 0.7199 }),
    (Seat { right_hand: true,  row: 4 }, Share { x0: 0.8880, y0: 0.7685, x1: 1.0000, y1: 0.8681 }),
];

/// Left, top, width, height in pixels.
pub type Box = (usize, usize, usize, usize);

/// The ten banners for a window of this size. A window too small to hold readable
/// letters gives nothing: at that size the glyphs are a few pixels tall and every read
/// would be a guess.
pub fn banners(w: usize, h: usize) -> Vec<(Seat, Box)> {
    if w < 1280 || h < 720 {
        return Vec::new();
    }
    BANNERS
        .iter()
        .filter_map(|(seat, s)| {
            let x0 = (s.x0 * w as f32).round() as usize;
            let y0 = (s.y0 * h as f32).round() as usize;
            let x1 = ((s.x1 * w as f32).round() as usize).min(w);
            let y1 = ((s.y1 * h as f32).round() as usize).min(h);
            (x1 > x0 && y1 > y0).then_some((*seat, (x0, y0, x1 - x0, y1 - y0)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boxes_match_the_capture_they_were_measured_from() {
        let found = banners(3840, 2160);
        assert_eq!(found.len(), 10);
        // The left panel's first row, padded out so no name is clipped at either end.
        let (_, first) = found[0];
        assert_eq!(first.0, 0, "{first:?}");
        assert!((first.1 as i32 - 320).abs() <= 4, "{first:?}");
        assert!((first.2 as i32 - 440).abs() <= 6, "{first:?}");
    }

    #[test]
    fn every_box_lands_inside_the_window() {
        for (w, h) in [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720)] {
            for (seat, (x, y, bw, bh)) in banners(w, h) {
                assert!(x + bw <= w && y + bh <= h, "{seat:?} {x} {y} {bw} {bh} in {w}x{h}");
                assert!(bw > 20 && bh > 20, "{seat:?} degenerate at {w}x{h}");
            }
        }
    }

    #[test]
    fn a_window_too_small_to_read_is_not_guessed_at() {
        assert!(banners(640, 360).is_empty());
    }
}
