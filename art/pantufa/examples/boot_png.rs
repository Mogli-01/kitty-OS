//! Renders frames of the boot animation to PNG files so the art can be
//! reviewed without booting: `cargo run -p kitty-pantufa --example boot_png`.
//! Output: `target/pantufa/boot_sheet.png` (9 frames) and `boot_<ms>.png`
//! at 3× scale.

use std::fs;
use std::io;
use std::path::Path;

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

/// Minimal PNG writer: 8-bit RGB, zlib with uncompressed (stored) blocks.
fn write_png(path: &Path, width: usize, height: usize, rgb: &[u8]) -> io::Result<()> {
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in rgb.chunks_exact(width * 3) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    for (kind, data) in [(b"IHDR", &ihdr), (b"IDAT", &zlib), (b"IEND", &Vec::new())] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }
    fs::write(path, png)
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &d in data {
        a = (a + u32::from(d)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &d in data {
        crc ^= u32::from(d);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
