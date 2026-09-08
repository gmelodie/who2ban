//! What `learn` would file from one draft screenshot, given the true names.
//!
//! The offline half of `Reader::harvest`: crop the ten banners, hand each one the name
//! the battlelobby would have given it, and report what the atlas gains.
//!
//!   cargo run -p w2b-glyph --example harvest -- shot.png atlas.json -- Name1 Name2 ...

use w2b_glyph::{Atlas, geometry, learn};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (head, truth) = args.split_at(split);
    let truth: Vec<&str> = truth.iter().skip(1).map(String::as_str).collect();

    let mut atlas = head
        .get(1)
        .filter(|p| p.as_str() != "-")
        .map(|p| Atlas::load(std::path::Path::new(p)).expect("atlas"))
        .unwrap_or_else(Atlas::new);

    let image = image::open(&head[0]).expect("open").to_rgb8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgb = image.into_raw();

    println!(
        "before: {} letters, {} examples",
        atlas.letters(),
        atlas.examples()
    );
    let mut filed = 0;
    for (i, (_, (x, y, bw, bh))) in geometry::banners(w, h).into_iter().enumerate() {
        let Some(&want) = truth.get(i) else { continue };
        let cut = crop(&rgb, w, x, y, bw, bh);
        let before = atlas.examples();
        let ok = learn(&cut, bw, bh, want, &mut atlas);
        filed += usize::from(ok);
        println!(
            "  seat {i} {want:20} {} {:+} examples",
            if ok { "FILED " } else { "passed" },
            atlas.examples() as i64 - before as i64
        );
    }
    println!(
        "after:  {} letters, {} examples ({filed}/10 banners filed)",
        atlas.letters(),
        atlas.examples()
    );
}

fn crop(rgb: &[u8], stride: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 3);
    for row in y..y + h {
        let start = (row * stride + x) * 3;
        out.extend_from_slice(&rgb[start..start + w * 3]);
    }
    out
}
