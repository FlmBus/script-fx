//! Renders a line of text in a patch's SVG/BMFont font to PNG.
//! `cargo run -p ui-components --example glyph_font -- <patch dir> <font> "<text>" out.png [size] [#rrggbb]`
use std::path::Path;

use resvg::tiny_skia::Color;
use ui_components::glyph_font::GlyphFont;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("usage: <patch dir> <font> <text> <out.png> [size] [#rrggbb]");
        std::process::exit(2);
    }
    let dir = Path::new(&a[0]);
    let font = GlyphFont::load(&dir.join(&a[1]), dir).unwrap_or_else(|e| panic!("{e}"));
    let size = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(48.0);
    let hex = a.get(5).map(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap()).unwrap_or(0x222222);
    let color = Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255);
    let pm = font.render(&a[2], size, color).expect("render");
    pm.save_png(&a[3]).unwrap();
    println!("{}x{} → {}", pm.width(), pm.height(), a[3]);
}
