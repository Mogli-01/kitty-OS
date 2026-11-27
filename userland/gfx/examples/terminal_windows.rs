use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write as _};
use std::path::Path;
use std::process::ExitCode;

use kitty_gfx::font::Psf2Font;
use kitty_gfx::{Canvas, Color, DoubleBuffer, Point, Rect, Surface, TextGrid, WindowStack};

const WIDTH: u32 = 384;
const HEIGHT: u32 = 192;
const FRAME_BYTES: usize = (WIDTH * HEIGHT * 4) as usize;
const MAX_FONT_BYTES: u64 = 4 * 1024 * 1024;
const INK: Color = Color::rgb(244, 237, 231);
const DESKTOP: Color = Color::rgb(22, 27, 39);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("terminal_windows: {reason}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let (Some(font_path), Some(directory), None) = (args.next(), args.next(), args.next()) else {
        return Err("usage: terminal_windows FONT.psfu OUTPUT_DIRECTORY".into());
    };
    let mut bytes = Vec::new();
    File::open(font_path)
        .and_then(|file| file.take(MAX_FONT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("reading font: {error}"))?;
    if bytes.len() as u64 > MAX_FONT_BYTES {
        return Err("font exceeds 4 MiB".into());
    }
    let font = Psf2Font::parse(&bytes).map_err(|error| format!("invalid PSF2 font: {error:?}"))?;
    let (glyph_width, glyph_height) = font.bitmap().dimensions();
    if glyph_width > 12
        || glyph_height > 24
        || font.glyph_for_char('é').is_none()
        || font.glyph_for_char('ã').is_none()
        || font.glyph_for_char('ç').is_none()
    {
        return Err("use a PSF2 font up to 12 × 24 pixels with é, ã and ç".into());
    }

    let mut terminal = [0; 240 * 96 * 4];
    let mut notice = [0; 156 * 72 * 4];
    let mut layers = [
        Canvas::new(&mut terminal, 240, 96, 240 * 4).map_err(|error| format!("{error:?}"))?,
        Canvas::new(&mut notice, 156, 72, 156 * 4).map_err(|error| format!("{error:?}"))?,
    ];
    draw_text(
        &mut layers[0],
        font,
        "Terminal\n$ echo café\ncafé",
        20,
        4,
        Color::rgb(37, 56, 79),
    )?;
    draw_text(
        &mut layers[1],
        font,
        "Janela\nAção",
        13,
        3,
        Color::rgb(103, 49, 71),
    )?;

    let mut front = vec![0; FRAME_BYTES];
    let mut back = vec![0; FRAME_BYTES];
    let mut stack = WindowStack::new(WIDTH, HEIGHT).map_err(|error| format!("{error:?}"))?;
    stack
        .insert(0, rectangle(24, 24, 240, 96))
        .map_err(|error| format!("{error:?}"))?;
    stack
        .insert(1, rectangle(176, 62, 156, 72))
        .map_err(|error| format!("{error:?}"))?;
    stack.damage_mut().invalidate_all();

    let directory = Path::new(&directory);
    fs::create_dir_all(directory).map_err(|error| format!("creating output directory: {error}"))?;
    present(&mut stack, &layers, &mut front, &mut back)?;
    write_ppm(&directory.join("before.ppm"), &front)?;
    stack
        .move_to(1, Point { x: 214, y: 108 })
        .map_err(|error| format!("{error:?}"))?;
    present(&mut stack, &layers, &mut front, &mut back)?;
    write_ppm(&directory.join("after.ppm"), &front)
}

fn draw_text(
    canvas: &mut Canvas<'_>,
    font: Psf2Font<'_>,
    text: &str,
    columns: u32,
    rows: u32,
    background: Color,
) -> Result<(), String> {
    let surface = canvas.as_surface();
    let bounds = rectangle(0, 0, surface.width(), surface.height());
    canvas.fill_rect(bounds, background);
    let mut cells = [' '; 80];
    let mut grid =
        TextGrid::new(&mut cells, columns, rows).map_err(|error| format!("{error:?}"))?;
    grid.write_str(text).map_err(|error| error.to_string())?;
    grid.draw_psf2(canvas, Point { x: 0, y: 0 }, font, INK, background);
    Ok(())
}

fn present(
    stack: &mut WindowStack,
    layers: &[Canvas<'_>; 2],
    front: &mut [u8],
    back: &mut [u8],
) -> Result<(), String> {
    let front = Canvas::new(front, WIDTH, HEIGHT, WIDTH as usize * 4)
        .map_err(|error| format!("{error:?}"))?;
    let back = Canvas::new(back, WIDTH, HEIGHT, WIDTH as usize * 4)
        .map_err(|error| format!("{error:?}"))?;
    let mut frame = DoubleBuffer::new(front, back).map_err(|error| format!("{error:?}"))?;
    for &changed in stack.damage().regions() {
        frame.draw().fill_rect(changed, DESKTOP);
        for window in stack.windows() {
            if let Some(visible) = window.bounds.intersection(changed) {
                let source: Surface<'_> = layers
                    .get(window.id as usize)
                    .ok_or("unknown demo layer")?
                    .as_surface();
                let x = visible.origin.x - window.bounds.origin.x;
                let y = visible.origin.y - window.bounds.origin.y;
                frame.draw().blend(
                    source,
                    rectangle(x, y, visible.width, visible.height),
                    visible.origin,
                );
            }
        }
    }
    frame
        .present_damage(stack.damage_mut())
        .map_err(|error| format!("{error:?}"))
}

fn rectangle(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        origin: Point { x, y },
        width,
        height,
    }
}

fn write_ppm(path: &Path, frame: &[u8]) -> Result<(), String> {
    let write = || -> std::io::Result<()> {
        let mut output = BufWriter::new(File::options().write(true).create_new(true).open(path)?);
        write!(output, "P6\n{WIDTH} {HEIGHT}\n255\n")?;
        for &[blue, green, red, _] in frame.as_chunks::<4>().0 {
            output.write_all(&[red, green, blue])?;
        }
        output.flush()
    };
    write().map_err(|error| format!("writing {}: {error}", path.display()))
}
