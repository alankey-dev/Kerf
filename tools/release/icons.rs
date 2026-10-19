//! Renders `tools/release/icon.svg` into every icon file the release packages need, so packaging does
//! not depend on `qlmanage` or `iconutil`. Run it after changing the SVG and commit what it writes:
//!
//! ```sh
//! cargo run --example icons
//! ```
//!
//! It writes `tools/release/icons/Kerf.icns` (macOS), `kerf.ico` (Windows) and `kerf-<size>.png` from 16
//! to 512 px (Linux, one per hicolor size).

use std::path::Path;

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, TreeParsing};

/// The hicolor sizes Linux desktops look up.
const LINUX: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// Windows picks the closest of these for the taskbar, Start menu, Explorer and title bar.
const WINDOWS: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
/// `.icns` element types that hold a PNG, with their pixel size. The `@2x` types share a size with a 1x one.
const MACOS: [(&[u8; 4], u32); 10] =
    [(b"icp4", 16), (b"icp5", 32), (b"ic11", 32), (b"ic12", 64), (b"ic07", 128), (b"ic08", 256), (b"ic13", 256), (b"ic09", 512), (b"ic14", 512), (b"ic10", 1024)];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/release");
    let svg = std::fs::read(root.join("icon.svg"))?;
    let tree = resvg::Tree::from_usvg(&usvg::Tree::from_data(&svg, &usvg::Options::default())?);
    let png = |size: u32| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut pixmap = Pixmap::new(size, size).ok_or("zero-sized icon")?;
        let scale = size as f32 / tree.size.width();
        tree.render(Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        Ok(pixmap.encode_png()?)
    };

    let out = root.join("icons");
    std::fs::create_dir_all(&out)?;
    for size in LINUX {
        std::fs::write(out.join(format!("kerf-{size}.png")), png(size)?)?;
    }
    let ico = WINDOWS.iter().map(|&s| Ok((s, png(s)?))).collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    std::fs::write(out.join("kerf.ico"), ico_file(&ico))?;
    let icns = MACOS.iter().map(|&(kind, s)| Ok((kind, png(s)?))).collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    std::fs::write(out.join("Kerf.icns"), icns_file(&icns))?;
    println!("Wrote {} icon files to {}", LINUX.len() + 2, out.display());
    Ok(())
}

/// An ICO file with one PNG entry per size. A 256 px entry stores its width and height as 0.
fn ico_file(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = images.len() as u16;
    let mut head = [0u16, 1, count].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in images {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        head.extend([side, side, 0, 0]);
        head.extend(1u16.to_le_bytes()); // colour planes
        head.extend(32u16.to_le_bytes()); // bits per pixel
        head.extend((png.len() as u32).to_le_bytes());
        head.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    images.iter().fold(head, |mut file, (_, png)| {
        file.extend(png);
        file
    })
}

/// An ICNS file: the `icns` magic and total length, then one big-endian `type, length, PNG` element each.
fn icns_file(images: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body: Vec<u8> = images.iter().flat_map(|(kind, png)| kind.iter().copied().chain((png.len() as u32 + 8).to_be_bytes()).chain(png.iter().copied())).collect();
    let mut file = b"icns".to_vec();
    file.extend((body.len() as u32 + 8).to_be_bytes());
    file.extend(body);
    file
}
