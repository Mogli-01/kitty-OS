//! Renders frames of the boot animation to PNG files so the art can be
//! reviewed without booting: `cargo run -p kitty-pantufa --example boot_png`.
//! Output: `target/pantufa/boot_sheet.png` (9 frames) and `boot_<ms>.png`
//! at 3× scale.

#[path = "common/png.rs"]
mod png;

use std::fs;
use std::io;
use std::path::Path;

use png::write_png;

use kitty_pantufa::{BootSplash, Canvas, SCENE_HEIGHT, SCENE_WIDTH, Target, palette, present_bgra};

const MOMENTS: [u32; 9] = [600, 1100, 1420, 1680, 1850, 2150, 2400, 2700, 3050];

fn main() -> io::Result<()> {
    let out = Path::new("target/pantufa");
    fs::create_dir_all(out)?;

    let (cols, rows) = (3, 3);
    let (sheet_w, sheet_h) = (SCENE_WIDTH * cols, SCENE_HEIGHT * rows);
    let mut sheet = vec![0u8; sheet_w * sheet_h * 3];

    let mut px = vec![0u8; SCENE_WIDTH * SCENE_HEIGHT];
    let mut splash = BootSplash::new();
    let mut captured = 0;
    let mut t = 0;
    while captured < MOMENTS.len() {
        let mut scene = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut px).expect("scene size");
        let stages = (t / 250).min(8) as u8;
        let frame = splash.draw(&mut scene, t, stages, t >= 1800);
        if t == MOMENTS[captured] {
            let (cx, cy) = (
                (captured % cols) * SCENE_WIDTH,
                (captured / cols) * SCENE_HEIGHT,
            );
            for (i, &index) in scene.pixels().iter().enumerate() {
                let [r, g, b] = palette::rgb(index, frame.brightness);
                let (x, y) = (cx + i % SCENE_WIDTH, cy + i / SCENE_WIDTH);
                sheet[(y * sheet_w + x) * 3..][..3].copy_from_slice(&[r, g, b]);
            }
            let (w, h) = (SCENE_WIDTH * 3, SCENE_HEIGHT * 3);
            let mut fb = vec![0u8; w * h * 4];
            present_bgra(
                &scene,
                frame.brightness,
                &mut fb,
                Target {
                    width: w,
                    height: h,
                    pitch: w * 4,
                },
            )
            .expect("present");
            let rgb: Vec<u8> = fb
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0]])
                .collect();
            write_png(&out.join(format!("boot_{t}.png")), w, h, &rgb)?;
            captured += 1;
        }
        t += 10;
    }
    write_png(&out.join("boot_sheet.png"), sheet_w, sheet_h, &sheet)?;
    println!("wrote {} frames to {}", MOMENTS.len(), out.display());
    Ok(())
}
