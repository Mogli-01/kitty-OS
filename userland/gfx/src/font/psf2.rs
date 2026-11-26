//! Read-only views of PSF2 version 0 fonts.

use core::str;

use super::{BitmapFont, Glyph};

const MAGIC: [u8; 4] = [0x72, 0xb5, 0x4a, 0x86];
const HEADER_BYTES: usize = 32;
const HAS_UNICODE_TABLE: u32 = 1;
const GLYPH_SEPARATOR: u8 = 0xff;
const START_SEQUENCE: u8 = 0xfe;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Psf2Error {
    InvalidHeader,
    UnsupportedFormat,
    InvalidGlyphs,
    InvalidUnicodeTable,
}

/// A validated atlas and optional Unicode map borrowed from a PSF2 file.
#[derive(Clone, Copy)]
pub struct Psf2Font<'a> {
    bitmap: BitmapFont<'a>,
    unicode: Option<&'a [u8]>,
}

impl<'a> Psf2Font<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, Psf2Error> {
        let header = file.get(..HEADER_BYTES).ok_or(Psf2Error::InvalidHeader)?;
        if header[..MAGIC.len()] != MAGIC {
            return Err(Psf2Error::InvalidHeader);
        }
        let words = header.as_chunks::<4>().0;
        let version = u32::from_le_bytes(words[1]);
        let header_size =
            usize::try_from(u32::from_le_bytes(words[2])).map_err(|_| Psf2Error::InvalidHeader)?;
        let flags = u32::from_le_bytes(words[3]);
        if version != 0 || flags & !HAS_UNICODE_TABLE != 0 {
            return Err(Psf2Error::UnsupportedFormat);
        }
        if header_size < HEADER_BYTES {
            return Err(Psf2Error::InvalidHeader);
        }
        let payload = file.get(header_size..).ok_or(Psf2Error::InvalidHeader)?;

        let count =
            usize::try_from(u32::from_le_bytes(words[4])).map_err(|_| Psf2Error::InvalidGlyphs)?;
        let stored_size = u32::from_le_bytes(words[5]);
        let height =
            u8::try_from(u32::from_le_bytes(words[6])).map_err(|_| Psf2Error::InvalidGlyphs)?;
        let width =
            u8::try_from(u32::from_le_bytes(words[7])).map_err(|_| Psf2Error::InvalidGlyphs)?;
        let glyph_size = u32::from(width).div_ceil(8) * u32::from(height);
        if stored_size != glyph_size {
            return Err(Psf2Error::InvalidGlyphs);
        }
        let atlas_size = count
            .checked_mul(glyph_size as usize)
            .ok_or(Psf2Error::InvalidGlyphs)?;
        let (glyphs, trailer) = payload
            .split_at_checked(atlas_size)
            .ok_or(Psf2Error::InvalidGlyphs)?;
        let bitmap =
            BitmapFont::new(glyphs, width, height).map_err(|_| Psf2Error::InvalidGlyphs)?;

        let unicode = if flags & HAS_UNICODE_TABLE != 0 {
            validate_unicode(trailer, count)?;
            Some(trailer)
        } else if trailer.is_empty() {
            None
        } else {
            return Err(Psf2Error::InvalidGlyphs);
        };
        Ok(Self { bitmap, unicode })
    }

    /// Returns the bitmap atlas for numeric glyph indices.
    pub fn bitmap(self) -> BitmapFont<'a> {
        self.bitmap
    }

    /// Returns the first glyph declaring this scalar as an alias. Fonts with
    /// no Unicode map have no aliases. Multi-character sequences do not act
    /// as aliases for their constituent scalars.
    pub fn glyph_for_char(self, character: char) -> Option<Glyph<'a>> {
        let table = self.unicode?;
        let index = table
            .split(|&byte| byte == GLYPH_SEPARATOR)
            .position(|record| {
                let aliases = record.split(|&byte| byte == START_SEQUENCE).next();
                aliases
                    .and_then(|bytes| str::from_utf8(bytes).ok())
                    .is_some_and(|text| text.chars().any(|alias| alias == character))
            })?;
        self.bitmap.glyph(index)
    }
}

fn validate_unicode(table: &[u8], glyph_count: usize) -> Result<(), Psf2Error> {
    let table = table
        .strip_suffix(&[GLYPH_SEPARATOR])
        .ok_or(Psf2Error::InvalidUnicodeTable)?;
    let mut records = table.split(|&byte| byte == GLYPH_SEPARATOR);
    for _ in 0..glyph_count {
        let record = records.next().ok_or(Psf2Error::InvalidUnicodeTable)?;
        for (index, part) in record.split(|&byte| byte == START_SEQUENCE).enumerate() {
            if str::from_utf8(part).is_err() || (index != 0 && part.is_empty()) {
                return Err(Psf2Error::InvalidUnicodeTable);
            }
        }
    }
    if records.next().is_some() {
        return Err(Psf2Error::InvalidUnicodeTable);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
