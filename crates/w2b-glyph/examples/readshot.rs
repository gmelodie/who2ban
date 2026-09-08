//! What the draft reader makes of one screenshot, seat by seat.
//!
//! Runs the same ladder, the same ranking and the same thresholds the client does, so a
//! draft that read poorly can be asked why afterwards instead of guessed at live.
//!
//!   cargo run -p w2b-glyph --example readshot -- <shot.png> <atlas.json> <pool.json>

use w2b_glyph::{Atlas, geometry, name};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [shot, atlas_path, pool_path] = &args[..] else {
        eprintln!("usage: readshot <shot.png> <atlas.json> <pool.json>");
        std::process::exit(2);
    };

    let atlas: Atlas = serde_json::from_reader(std::fs::File::open(atlas_path).unwrap()).unwrap();
    let tags: Vec<String> =
        serde_json::from_reader(std::fs::File::open(pool_path).unwrap()).unwrap();
    let candidates: Vec<(String, String)> = tags
        .iter()
        .map(|t| {
            let n = t.split_once('#').map_or(t.as_str(), |(n, _)| n);
            (n.to_string(), t.clone())
        })
        .collect();

    let img = image::open(shot).unwrap().to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    println!(
        "shot {w}x{h} | atlas {} letters | pool {}",
        atlas.letters(),
        tags.len()
    );

    for (seat, (x, y, bw, bh)) in geometry::banners(w, h) {
        let mut rgb = Vec::with_capacity(bw * bh * 3);
        for row in y..y + bh {
            for col in x..x + bw {
                rgb.extend_from_slice(&img.get_pixel(col as u32, row as u32).0);
            }
        }
        let side = if seat.right_hand { "right" } else { "left " };
        let ladder = w2b_glyph::read_ladder(&rgb, bw, bh, &atlas);
        if ladder.is_empty() {
            println!("{side} row{} | nothing cut out of the banner", seat.row);
            continue;
        }
        // The rung the client would keep: the one whose text ranks best against the pool.
        let mut best: Option<(f32, String, usize)> = None;
        for r in &ladder {
            if let Some((found, _)) = name::rank(&r.text, &candidates)
                && best.as_ref().is_none_or(|(had, _, _)| found.score < *had)
            {
                best = Some((found.score, r.text.clone(), r.unread));
            }
        }
        let Some((_, text, unread)) = best else {
            let r = ladder.iter().min_by_key(|r| r.unread).unwrap();
            println!(
                "{side} row{} | read {:?} ({} holes) | ranks against nobody",
                seat.row, r.text, r.unread
            );
            continue;
        };
        let (found, runner) = name::rank(&text, &candidates).unwrap();
        let verdict = match name::identify(&text, &candidates) {
            Some(f) => format!("SEATED {}", f.battletag),
            None if !name::legible(&text) => "refused: read not legible".to_string(),
            // Checked in the order `identify` checks them, or the reason printed is not
            // the reason the seat is empty.
            None if found.shared > 1 => format!(
                "refused: {} players go by {:?}",
                found.shared,
                found
                    .battletag
                    .split_once('#')
                    .map_or(found.battletag.as_str(), |(n, _)| n)
            ),
            None if found.alone => format!(
                "refused: alone but score {:.3} > {:.2}",
                found.score,
                name::LONE_MAX_SCORE
            ),
            None if found.score > name::MAX_SCORE => {
                format!("refused: score {:.3} > {:.2}", found.score, name::MAX_SCORE)
            }
            None => format!(
                "refused: margin {:.3} < {:.2} (tie with {runner:?})",
                found.margin,
                name::MIN_MARGIN
            ),
        };
        println!(
            "{side} row{} | read {text:?} ({unread} holes) | best {:?} score {:.3} margin {:.3} | {verdict}",
            seat.row, found.battletag, found.score, found.margin
        );
    }
}
