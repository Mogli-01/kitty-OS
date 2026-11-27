extern crate std;

use std::vec;

use crate::palette::idx::{CREAM, FUR, NIGHT, OUTLINE, WALL};
use crate::palette::{self, TRANSPARENT};
use crate::{
    BOOT_LENGTH_MS, BootSplash, Canvas, CatSprite, CatStyle, Eyes, Mouth, Pose, PresentError,
    SCENE_HEIGHT, SCENE_WIDTH, Target, draw_cat, has_glyph, present_bgra, text_width,
};

fn scene() -> std::vec::Vec<u8> {
    vec![TRANSPARENT; SCENE_WIDTH * SCENE_HEIGHT]
}

#[test]
fn canvas_rejects_bad_sizes() {
    let mut small = [0u8; 10];
    assert!(Canvas::new(4, 4, &mut small).is_none());
    assert!(Canvas::new(0, 4, &mut small).is_none());
    assert!(Canvas::new(usize::MAX, 2, &mut small).is_none());
    assert!(Canvas::new(2, 5, &mut small).is_some());
}

#[test]
fn writes_outside_the_canvas_are_clipped() {
    let mut px = [TRANSPARENT; 16];
    let mut c = Canvas::new(4, 4, &mut px).unwrap();
    c.set(-1, 0, 1);
    c.set(4, 0, 1);
    c.rect(-100, -100, 1000, 2, 1);
    c.ellipse(1.0e6, 1.0e6, 1.0e6, 1.0e6, 1);
    c.line((-1.0e9, 0.0), (1.0e9, 0.0), 1);
    c.triangle([(-50.0, -50.0), (50.0, -50.0), (0.0, 50.0)], 2);
    assert_eq!(c.get(-1, 0), TRANSPARENT);
    assert_eq!(c.get(0, 4), TRANSPARENT);
    assert!(c.pixels().iter().all(|&p| p == 2));
}

#[test]
fn ellipse_centred_on_a_pixel_edge_is_symmetric() {
    let mut px = [TRANSPARENT; 20 * 20];
    let mut c = Canvas::new(20, 20, &mut px).unwrap();
    c.ellipse(10.0, 10.0, 6.5, 4.2, 3);
    for y in 0..20 {
        for x in 0..20 {
            assert_eq!(c.get(x, y), c.get(19 - x, y), "({x}, {y})");
            assert_eq!(c.get(x, y), c.get(x, 19 - y), "({x}, {y})");
        }
    }
    assert_eq!(c.get(10, 10), 3);
}

#[test]
fn outline_touches_only_the_four_neighbours() {
    let mut px = [TRANSPARENT; 5 * 5];
    let mut c = Canvas::new(5, 5, &mut px).unwrap();
    c.set(2, 2, 9);
    c.outline(OUTLINE);
    assert_eq!(c.get(2, 2), 9);
    for (x, y) in [(1, 2), (3, 2), (2, 1), (2, 3)] {
        assert_eq!(c.get(x, y), OUTLINE);
    }
    for (x, y) in [(1, 1), (3, 3), (0, 0), (4, 2)] {
        assert_eq!(c.get(x, y), TRANSPARENT);
    }
}

#[test]
fn flipped_blit_mirrors_and_skips_transparent_pixels() {
    let mut src_px = [TRANSPARENT; 3];
    let mut src = Canvas::new(3, 1, &mut src_px).unwrap();
    src.set(0, 0, 4);
    let mut dst_px = [7u8; 3];
    let mut dst = Canvas::new(3, 1, &mut dst_px).unwrap();
    dst.blit(&src, 0, 0, true);
    assert_eq!(dst.pixels(), &[7, 7, 4]);
}

#[test]
fn darker_palette_keeps_transparency() {
    let mut px = [CREAM, TRANSPARENT];
    let mut c = Canvas::new(2, 1, &mut px).unwrap();
    c.remap(&palette::DARKER);
    assert_eq!(c.pixels(), &[palette::DARKER[CREAM as usize], TRANSPARENT]);
}

#[test]
fn font_covers_the_boot_screen_text() {
    for s in [
        "KITTY-OS",
        "A ACORDAR: INTERRUPCOES",
        "PRONTO!",
        "MIAU!",
        "Até amanhã. Miau.",
        "0123456789:?,",
    ] {
        assert!(s.chars().all(has_glyph), "{s}");
    }
    assert_eq!(text_width("", 2), 0);
    assert_eq!(text_width("OK", 1), 11);
    assert_eq!(text_width("OK", 2), 22);
}

#[test]
fn every_pose_draws_an_outlined_cat() {
    let mut px = scene();
    let mut c = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut px).unwrap();
    let mut sprite = CatSprite::new();
    for pose in [Pose::Sit, Pose::Curl, Pose::Stretch, Pose::Ball] {
        for eyes in [
            Eyes::Closed,
            Eyes::Open,
            Eyes::Half,
            Eyes::Happy,
            Eyes::Wide,
            Eyes::OneOpen,
        ] {
            c.clear(TRANSPARENT);
            let style = CatStyle {
                eyes,
                mouth: Mouth::Yawn,
                tail: 1.5,
                ..CatStyle::new(pose)
            };
            draw_cat(&mut c, &mut sprite, 160.0, 120.0, &style);
            assert!(c.pixels().contains(&FUR), "{pose:?}");
            assert!(c.pixels().contains(&OUTLINE), "{pose:?}");
            // The cat stands on its feet: nothing below the anchor's row.
            assert!(
                (122..SCENE_HEIGHT as i32).all(|y| (0..320).all(|x| c.get(x, y) == TRANSPARENT))
            );
        }
    }
    // Drawn far off-screen: clipped, no panic.
    draw_cat(
        &mut c,
        &mut sprite,
        -500.0,
        9999.0,
        &CatStyle::new(Pose::Sit),
    );
}

fn run(
    splash: &mut BootSplash,
    c: &mut Canvas<'_>,
    until: u32,
    ready_at: Option<u32>,
) -> Option<u32> {
    let mut t = 0;
    while t <= until {
        let stages = (t / 250).min(8) as u8;
        let ready = ready_at.is_some_and(|r| t >= r);
        let frame = splash.draw(c, t, stages, ready);
        assert!(frame.brightness <= 4);
        if frame.finished {
            return Some(t);
        }
        t += 10;
    }
    None
}

#[test]
fn boot_waits_for_the_system_before_jumping() {
    let mut px = scene();
    let mut c = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut px).unwrap();
    let mut splash = BootSplash::new();
    assert_eq!(
        run(&mut splash, &mut c, 10_000, None),
        None,
        "never ready: never finishes"
    );

    let mut splash = BootSplash::new();
    assert_eq!(
        run(&mut splash, &mut c, 10_000, Some(0)),
        Some(BOOT_LENGTH_MS),
        "ready early: 3.3 s"
    );

    let mut splash = BootSplash::new();
    let late = 5_000;
    let end = run(&mut splash, &mut c, 20_000, Some(late)).unwrap();
    assert_eq!(
        end,
        late + BOOT_LENGTH_MS - 1950,
        "the jump starts when ready"
    );
}

#[test]
fn paw_prints_follow_the_reported_stages() {
    let mut px = scene();
    let mut c = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut px).unwrap();
    let mut splash = BootSplash::new();
    splash.draw(&mut c, 600, 1, false);
    // First print (lit), second print still a placeholder dot.
    assert_eq!(c.get(107, 149), FUR);
    assert_eq!(c.get(121 + 2, 152 + 3), WALL);
}

#[test]
fn present_scales_centres_and_fades() {
    let mut px = scene();
    let c = {
        let mut c = Canvas::new(SCENE_WIDTH, SCENE_HEIGHT, &mut px).unwrap();
        c.clear(CREAM);
        c
    };
    let (w, h, pitch) = (1000, 600, 1000 * 4 + 16);
    let mut fb = vec![0xAAu8; pitch * h];
    let scale = present_bgra(
        &c,
        4,
        &mut fb,
        Target {
            width: w,
            height: h,
            pitch,
        },
    )
    .unwrap();
    assert_eq!(scale, 3);
    let pixel = |fb: &[u8], x: usize, y: usize| -> [u8; 4] {
        fb[y * pitch + x * 4..][..4].try_into().unwrap()
    };
    assert_eq!(pixel(&fb, 500, 300), palette::bgra(CREAM, 4));
    assert_eq!(pixel(&fb, 0, 0), palette::bgra(NIGHT, 4));
    assert_eq!(&fb[w * 4..pitch], &[0xAA; 16], "row padding is preserved");

    present_bgra(
        &c,
        0,
        &mut fb,
        Target {
            width: w,
            height: h,
            pitch,
        },
    )
    .unwrap();
    assert_eq!(pixel(&fb, 500, 300), [0, 0, 0, 0xFF]);

    let bad = Target {
        width: w,
        height: h,
        pitch: 10,
    };
    assert_eq!(
        present_bgra(&c, 4, &mut fb, bad),
        Err(PresentError::BadTarget)
    );
    let short = Target {
        width: w,
        height: h + 1,
        pitch,
    };
    assert_eq!(
        present_bgra(&c, 4, &mut fb, short),
        Err(PresentError::BadTarget)
    );
    let tiny = Target {
        width: 100,
        height: 100,
        pitch: 400,
    };
    assert_eq!(
        present_bgra(&c, 4, &mut fb, tiny),
        Err(PresentError::TooSmall)
    );
}

/// Hostile inputs from the security review: none may panic (debug builds
/// check overflow) or loop for long.
#[test]
fn hostile_coordinates_are_safe() {
    let mut px = [TRANSPARENT; 16 * 16];
    let mut c = Canvas::new(16, 16, &mut px).unwrap();
    for v in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -3.0e9,
        3.0e9,
        -2_147_483_648.0,
        2_147_483_648.0,
    ] {
        c.set_at(v, v, 1);
        c.ellipse(v, 0.0, 1.0, 1.0, 1);
        c.ellipse(0.0, 0.0, v, 1.0, 1);
        c.ellipse_rotated(4.0, 4.0, 2.0, 2.0, v, 1);
        c.ring(v, v, v, v, 0.5, 1);
        c.triangle([(v, 0.0), (0.0, v), (v, v)], 1);
        c.line((v, 0.0), (0.0, 0.0), 1);
        c.line((v, v), (0.0, 3.0), 1);
    }
    let mut sprite_px = [1u8; 4];
    let sprite = Canvas::new(2, 2, &mut sprite_px).unwrap();
    c.blit(&sprite, i32::MAX, i32::MAX, true);
    c.blit(&sprite, i32::MIN, i32::MIN, false);
    c.grid(&["11"], i32::MAX, i32::MAX, None);
    assert_eq!(
        crate::text(&mut c, "A", i32::MAX, 0, 1, i32::MAX),
        text_width("A", crate::MAX_TEXT_SCALE)
    );
    crate::text(&mut c, "AB", i32::MIN, i32::MIN, 1, i32::MIN);
    assert!(text_width("AB", 400_000_000) > 0);

    let mut cat = CatSprite::new();
    for v in [f32::NAN, f32::INFINITY, -1.0e10, 3.0e9] {
        let style = CatStyle {
            scale_x: v,
            scale_y: v,
            tail: v,
            stretch: v,
            ..CatStyle::new(Pose::Stretch)
        };
        draw_cat(&mut c, &mut cat, v, v, &style);
    }
}
