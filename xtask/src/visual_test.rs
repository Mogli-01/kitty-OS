//! Boots a graphical guest without a local display and checks the banner.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::{Opts, Result, TEST_TIMEOUT, build_dir, build_kernel, qemu_command};

const BOOT_COMPLETE: &str = "[boot] boot complete,";
const MAX_SERIAL: usize = 64 * 1024;
const MAX_SCREENSHOT: usize = 32 * 1024 * 1024;
const REFERENCE: &[u8] = include_bytes!("../../tests/visual/boot-banner.ppm");

pub(crate) fn run(opts: &Opts) -> Result<()> {
    let expected = Ppm::parse(REFERENCE).map_err(|error| format!("invalid reference: {error}"))?;
    let kernel = build_kernel(opts, false)?;
    let mut command = qemu_command(opts, &kernel)?;
    let output = build_dir().join("visual");
    fs::create_dir_all(&output).map_err(|e| format!("creating {}: {e}", output.display()))?;
    let serial = output.join("serial.log");
    let screenshot = output.join("frame.ppm");
    for path in [&serial, &screenshot] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cleaning {}: {e}", path.display())),
        }
    }
    let monitor = File::create(output.join("monitor.log"))
        .map_err(|e| format!("opening QEMU monitor log: {e}"))?;
    let mut guest = command
        .args([
            "-display",
            "none",
            "-monitor",
            "stdio",
            "-serial",
            "file:build/visual/serial.log",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(monitor))
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;
    let captured = capture(&mut guest, &serial, &screenshot);
    if captured.is_err() {
        let _ = guest.kill();
        let _ = guest.wait();
    }
    let bytes = captured?;
    let actual = Ppm::parse(&bytes)?;
    actual.check_banner(&expected)?;
    println!(
        "xtask: boot banner matches reference ({}x{} framebuffer)",
        actual.width, actual.height
    );
    println!("xtask: screenshot saved to {}", screenshot.display());
    Ok(())
}

fn capture(guest: &mut Child, serial: &Path, screenshot: &Path) -> Result<Vec<u8>> {
    let start = Instant::now();
    loop {
        if let Some(status) = guest
            .try_wait()
            .map_err(|e| format!("waiting for QEMU: {e}"))?
        {
            return Err(format!("QEMU exited before capture ({status})"));
        }
        match read_bounded(serial, MAX_SERIAL) {
            Ok(bytes) if String::from_utf8_lossy(&bytes).contains(BOOT_COMPLETE) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("reading {}: {e}", serial.display())),
        }
        if start.elapsed() >= TEST_TIMEOUT {
            return Err(format!(
                "timed out waiting for the boot message in {}",
                serial.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    monitor_write(guest, b"screendump build/visual/frame.ppm\n")?;
    let start = Instant::now();
    let bytes = loop {
        if let Some(status) = guest
            .try_wait()
            .map_err(|e| format!("waiting for QEMU: {e}"))?
        {
            return Err(format!(
                "QEMU exited before the screenshot was complete ({status})"
            ));
        }
        match read_bounded(screenshot, MAX_SCREENSHOT) {
            Ok(bytes) if Ppm::parse(&bytes).is_ok() => break bytes,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("reading {}: {e}", screenshot.display())),
        }
        if start.elapsed() >= TEST_TIMEOUT {
            return Err(format!(
                "timed out waiting for a complete P6 image at {}",
                screenshot.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    monitor_write(guest, b"quit\n")?;
    let start = Instant::now();
    while start.elapsed() < TEST_TIMEOUT {
        if let Some(status) = guest
            .try_wait()
            .map_err(|e| format!("waiting for QEMU: {e}"))?
        {
            return if status.success() {
                Ok(bytes)
            } else {
                Err(format!("QEMU capture failed ({status})"))
            };
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err("timed out waiting for QEMU to exit after the screen capture".into())
}

fn monitor_write(guest: &mut Child, command: &[u8]) -> Result<()> {
    let input = guest
        .stdin
        .as_mut()
        .ok_or("QEMU monitor input unavailable")?;
    input
        .write_all(command)
        .and_then(|()| input.flush())
        .map_err(|e| format!("writing to the QEMU monitor: {e}"))
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds capture size limit",
        ));
    }
    Ok(bytes)
}

struct Ppm<'a> {
    width: usize,
    height: usize,
    pixels: &'a [u8],
}

impl<'a> Ppm<'a> {
    // QEMU screendump writes one line each for magic, size and maxval.
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut lines = bytes.splitn(4, |&byte| byte == b'\n');
        if lines.next() != Some(b"P6") {
            return Err("expected a binary P6 screenshot".into());
        }
        let dimensions = lines.next().ok_or("missing PPM dimensions")?;
        let dimensions = std::str::from_utf8(dimensions).map_err(|_| "invalid PPM dimensions")?;
        let mut dimensions = dimensions.split_ascii_whitespace();
        let width = dimensions
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or("invalid PPM width")?;
        let height = dimensions
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or("invalid PPM height")?;
        if width == 0 || height == 0 || dimensions.next().is_some() || lines.next() != Some(b"255")
        {
            return Err("expected a non-empty PPM with 8-bit RGB channels".into());
        }
        let length = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or("PPM dimensions overflow")?;
        let pixels = lines.next().ok_or("missing PPM pixels")?;
        if pixels.len() != length {
            return Err("PPM pixel data length does not match the dimensions".into());
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    fn check_banner(&self, reference: &Self) -> Result<()> {
        if self.width < reference.width || self.height < reference.height {
            return Err("framebuffer is smaller than the reference banner".into());
        }
        let row_len = reference.width * 3;
        for (y, row) in reference.pixels.chunks_exact(row_len).enumerate() {
            let offset = y * self.width * 3;
            let actual = &self.pixels[offset..offset + row_len];
            if let Some(index) = actual
                .iter()
                .zip(row)
                .position(|(pixel, expected)| pixel != expected)
            {
                return Err(format!(
                    "boot banner differs at pixel ({}, {y}) in build/visual/frame.ppm",
                    index / 3
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_ppm_header_dimensions_and_length() {
        for bytes in [
            &b"P3\n1 1\n255\nRGB"[..],
            b"P6\n1 1\n1\nRGB",
            b"P6\n0 1\n255\n",
            b"P6\n1 1 1\n255\nRGB",
            b"P6\n1 2\n255\nRGB",
            b"P6\n1 1\n255\nRGBX",
        ] {
            assert!(Ppm::parse(bytes).is_err(), "{bytes:?}");
        }
        let huge = format!("P6\n{} 2\n255\n", usize::MAX);
        assert!(Ppm::parse(huge.as_bytes()).is_err());
        let image = Ppm::parse(b"P6\n1 1\n255\n\n\r\0").unwrap();
        assert_eq!(image.pixels, b"\n\r\0");
    }

    #[test]
    fn compares_upper_left_pixels_across_different_row_lengths() {
        let banner = Ppm::parse(b"P6\n1 2\n255\nabcdef").unwrap();
        let correct = Ppm::parse(b"P6\n2 2\n255\nabcXXXdefYYY").unwrap();
        assert!(correct.check_banner(&banner).is_ok());
        let wrong = Ppm::parse(b"P6\n2 2\n255\nabcXXXbadYYY").unwrap();
        assert!(wrong.check_banner(&banner).unwrap_err().contains("(0, 1)"));
        let small = Ppm::parse(b"P6\n1 1\n255\nabc").unwrap();
        assert!(small.check_banner(&banner).is_err());
    }
}
