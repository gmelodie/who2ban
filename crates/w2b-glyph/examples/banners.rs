//! What the segmenter makes of one draft screenshot, seat by seat.
//!
//! Point it at a capture of the game window and give it the ten names in lobby order,
//! and it reports, for every rung of the brightness ladder, how many shapes the banner
//! cut into against how many letters the name actually has. That difference is what
//! decides whether `learn` files the banner or throws it away.
//!
//!   cargo run -p w2b-glyph --example banners -- shot.png [atlas.json] -- Name1 Name2 ...

use w2b_glyph::{Atlas, BRIGHTNESS_LADDER, atlas as atlas_mod, geometry, letters_at, read_at};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (head, truth) = args.split_at(split);
    let truth: Vec<&str> = truth.iter().skip(1).map(String::as_str).collect();

    let shot = &head[0];
    let atlas = head
        .get(1)
        .map(|p| Atlas::load(std::path::Path::new(p)).expect("atlas"))
        .unwrap_or_else(Atlas::new);
    eprintln!(
        "atlas: {} letters, {} examples",
        atlas.letters(),
        atlas.examples()
    );

    let image = image::open(shot).expect("open").to_rgb8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgb = image.into_raw();
    eprintln!("window: {w}x{h}\n");

    for (i, (seat, (x, y, bw, bh))) in geometry::banners(w, h).into_iter().enumerate() {
        let want = truth.get(i).copied().unwrap_or("?");
        let letters = want.chars().filter(|c| !c.is_whitespace()).count();
        let cut = crop(&rgb, w, x, y, bw, bh);
        println!(
            "seat {i} {} row {} | {want:?} wants {letters}",
            if seat.right_hand { "R" } else { "L" },
            seat.row
        );
        for &t in BRIGHTNESS_LADDER.iter() {
            match letters_at(&cut, bw, bh, t) {
                None => println!("    {t:.2}: no baseline"),
                Some((blobs, angle)) => {
                    let read = read_at(&cut, bw, bh, &atlas, t)
                        .map(|r| r.text)
                        .unwrap_or_default();
                    let verdict = match blobs.len().cmp(&letters) {
                        std::cmp::Ordering::Equal => "FILES",
                        std::cmp::Ordering::Less => "short",
                        std::cmp::Ordering::Greater => "long",
                    };
                    println!(
                        "    {t:.2}: {} shapes ({verdict}) angle {angle:.1} read {read:?}",
                        blobs.len()
                    );
                    if verdict != "FILES" {
                        println!("         widths {:?}", widths(&blobs, angle));
                    }
                }
            }
        }
        println!();
    }
}

fn widths(blobs: &[w2b_glyph::segment::Blob], angle: f32) -> Vec<String> {
    blobs
        .iter()
        .map(|b| {
            let placed = atlas_mod::render(b, angle).is_some();
            format!(
                "{}x{}{}",
                b.width(),
                b.height(),
                if placed { "" } else { "!" }
            )
        })
        .collect()
}

fn crop(rgb: &[u8], stride: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 3);
    for row in y..y + h {
        let start = (row * stride + x) * 3;
        out.extend_from_slice(&rgb[start..start + w * 3]);
    }
    out
}
