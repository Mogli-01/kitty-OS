//! Renders the poses and the system scenes to PNG for review:
//! `cargo run -p kitty-pantufa --example scenes_png`.
//! Output in `target/pantufa/`: `poses.png` (all poses and heads, 3×) and
//! `scenes.png` (wallpaper in 3 skies, shutdown, kernel panic).

#[path = "common/png.rs"]
mod png;

use std::fs;
use std::io;
use std::path::Path;

use kitty_pantufa::{
    Canvas, CatSprite, CatStyle, Eyes, HeadEyes, HeadMouth, HeadSprite, HeadStyle, Mouth,
    PanicKind, PanicScene, Paws, Pose, Room, SCENE_HEIGHT, SCENE_WIDTH, Shutdown, Sky, draw_cat,
    draw_head, palette, text,
};
use png::write_png;

const W: usize = SCENE_WIDTH;
const H: usize = SCENE_HEIGHT;

/// Copies a scene into an RGB sheet at tile `(col, row)`, scaled.
fn paste(
    sheet: &mut [u8],
    sheet_w: usize,
    scene: &Canvas<'_>,
    col: usize,
    row: usize,
    scale: usize,
    quarters: u8,
) {
    for (i, &index) in scene.pixels().iter().enumerate() {
        let rgb = palette::rgb(index, quarters);
        let (x, y) = (i % W, i / W);
        for dy in 0..scale {
            for dx in 0..scale {
                let (px, py) = (
                    col * W * scale + x * scale + dx,
                    row * H * scale + y * scale + dy,
                );
                sheet[(py * sheet_w + px) * 3..][..3].copy_from_slice(&rgb);
            }
        }
    }
}

fn main() -> io::Result<()> {
    let out = Path::new("target/pantufa");
    fs::create_dir_all(out)?;
    let mut px = vec![0u8; W * H];

    // --- poses and heads ---
    {
        let mut c = Canvas::new(W, H, &mut px).expect("scene");
        c.clear(palette::idx::NIGHT);
        let mut cat = CatSprite::new();
        let poses = [
            (Pose::Sit, Eyes::Open, Mouth::Closed, "SIT"),
            (Pose::Back, Eyes::Closed, Mouth::Closed, "BACK"),
            (Pose::Curl, Eyes::Closed, Mouth::Closed, "CURL"),
            (Pose::Stretch, Eyes::Closed, Mouth::Closed, "STRETCH"),
            (Pose::Ball, Eyes::Happy, Mouth::Open, "BALL"),
            (Pose::Walk, Eyes::Open, Mouth::Closed, "WALK"),
            (Pose::Puff, Eyes::Wide, Mouth::Closed, "PUFF"),
        ];
        for (i, (pose, eyes, mouth, name)) in poses.into_iter().enumerate() {
            let x = 24.0 + i as f32 * 45.0;
            let style = CatStyle {
                eyes,
                mouth,
                puff: 1.0,
                step: 1,
                ..CatStyle::new(pose)
            };
            draw_cat(&mut c, &mut cat, x, 70.0, &style);
            text(&mut c, name, x as i32 - 14, 76, palette::idx::RAIN, 1);
        }
        let mut head = HeadSprite::new();
        let heads = [
            HeadStyle::new(),
            HeadStyle {
                look: (1, -1),
                ..HeadStyle::new()
            },
            HeadStyle {
                eyes: HeadEyes::Happy,
                mouth: HeadMouth::Open,
                ..HeadStyle::new()
            },
            HeadStyle {
                paws: Paws::Peek,
                ..HeadStyle::new()
            },
            HeadStyle {
                eyes: HeadEyes::Half,
                mouth: HeadMouth::Hmpf,
                ear_flat: true,
                ..HeadStyle::new()
            },
            HeadStyle {
                eyes: HeadEyes::Angry,
                mouth: HeadMouth::Hiss,
                ear_flat: true,
                bristle: true,
                ..HeadStyle::new()
            },
        ];
        for (i, style) in heads.iter().enumerate() {
            draw_head(&mut c, &mut head, 30.0 + i as f32 * 52.0, 125.0, 1.8, style);
        }
        draw_head(&mut c, &mut head, 300.0, 165.0, 0.62, &HeadStyle::new());
        let mut sheet = vec![0u8; W * 3 * H * 3 * 3];
        paste(&mut sheet, W * 3, &c, 0, 0, 3, 4);
        write_png(&out.join("poses.png"), W * 3, H * 3, &sheet)?;
    }

    // --- scenes: wallpaper ×3, shutdown ×3, panic ×3 ---
    let (cols, rows) = (3, 3);
    let mut sheet = vec![0u8; W * cols * H * rows * 3];
    let mut room = Box::new(Room::new());
    let mut cat = CatSprite::new();
    for (i, sky) in [Sky::Night, Sky::Sunset, Sky::Day].into_iter().enumerate() {
        let mut c = Canvas::new(W, H, &mut px).expect("scene");
        room.build(sky, true);
        room.draw(&mut c, 7_000);
        let (x, y) = Room::SOFA_SPOT;
        draw_cat(&mut c, &mut cat, x, y, &Room::sleeping_cat(7_000));
        paste(&mut sheet, W * cols, &c, i, 0, 1, 4);
    }
    let mut shutdown = Box::new(Shutdown::new());
    let mut col = 0;
    let mut t = 0;
    while col < 3 {
        let mut c = Canvas::new(W, H, &mut px).expect("scene");
        let frame = shutdown.draw(&mut c, t, true);
        if [800, 2100, 2650].contains(&t) {
            paste(&mut sheet, W * cols, &c, col, 1, 1, frame.brightness);
            col += 1;
        }
        t += 10;
    }
    let mut panic = Box::new(PanicScene::new());
    let kinds = [
        (
            PanicKind::PageFault,
            "0X0E",
            [
                "ERRO      PAGE_FAULT (0X0E)",
                "CR2 0X123A40   ESCRITA",
                "PILHA  #0 MAP_PAGE  #1 SPAWN  #2 KMAIN",
            ],
        ),
        (
            PanicKind::OutOfMemory,
            "OOM",
            [
                "ERRO      OUT_OF_MEMORY",
                "PEDIDO 4096 PAGINAS   LIVRE 0 KIB",
                "PILHA  #0 ALLOC_PAGES  #1 KMAIN",
            ],
        ),
        (
            PanicKind::DoubleFault,
            "0X08",
            [
                "ERRO      DOUBLE_FAULT (0X08)",
                "CAUSA     PAGE FAULT NO TRATADOR",
                "IST       PILHA DE EMERGENCIA 1",
            ],
        ),
    ];
    for (i, (kind, code, lines)) in kinds.into_iter().enumerate() {
        let mut c = Canvas::new(W, H, &mut px).expect("scene");
        panic.draw(&mut c, kind, code, &lines, 1_500);
        paste(&mut sheet, W * cols, &c, i, 2, 1, 4);
    }
    write_png(&out.join("scenes.png"), W * cols, H * rows, &sheet)?;
    println!("wrote poses.png and scenes.png to {}", out.display());
    Ok(())
}
