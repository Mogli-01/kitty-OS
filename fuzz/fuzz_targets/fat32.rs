#![no_main]

use kitty_fat32::{Fat32, Kind, ReadAt, ReadError};
use libfuzzer_sys::fuzz_target;

const SECTOR_BYTES: u64 = 512;
const FAT_SECTOR: u64 = 32;
const ROOT_SECTOR: u64 = FAT_SECTOR + 512;
const TOTAL_SECTORS: u64 = ROOT_SECTOR + 65_525;

// Keep the smallest valid FAT32 geometry while storing only sectors the
// corpus can affect. Every other in-volume sector is zero-filled.
struct Volume {
    boot: [u8; 512],
    fat: [u8; 512],
    clusters: [u8; 2048],
}

impl ReadAt for Volume {
    fn byte_len(&self) -> u64 {
        TOTAL_SECTORS * SECTOR_BYTES
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ReadError> {
        let end = offset.checked_add(out.len() as u64).ok_or(ReadError)?;
        if end > self.byte_len() {
            return Err(ReadError);
        }
        out.fill(0);
        for (start, bytes) in [
            (0, self.boot.as_slice()),
            (FAT_SECTOR * SECTOR_BYTES, self.fat.as_slice()),
            (ROOT_SECTOR * SECTOR_BYTES, self.clusters.as_slice()),
        ] {
            let first = start.max(offset);
            let last = (start + bytes.len() as u64).min(end);
            if first < last {
                out[(first - offset) as usize..(last - offset) as usize]
                    .copy_from_slice(&bytes[(first - start) as usize..(last - start) as usize]);
            }
        }
        Ok(())
    }
}

fn replace(target: &mut [u8], input: &[u8]) {
    let count = target.len().min(input.len());
    target[..count].copy_from_slice(&input[..count]);
}

fuzz_target!(|data: &[u8]| {
    let mut volume = Volume {
        boot: [0; 512],
        fat: [0; 512],
        clusters: [0; 2048],
    };
    volume.boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    volume.boot[13] = 1;
    volume.boot[14] = FAT_SECTOR as u8;
    volume.boot[16] = 1;
    volume.boot[32..36].copy_from_slice(&(TOTAL_SECTORS as u32).to_le_bytes());
    volume.boot[36..40].copy_from_slice(&512u32.to_le_bytes());
    volume.boot[44] = 2;
    volume.boot[510..512].copy_from_slice(&[0x55, 0xaa]);
    volume.fat[8..12].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
    volume.fat[12] = 5;
    volume.fat[20..24].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
    volume.clusters[..11].copy_from_slice(b"SAMPLE  BIN");
    volume.clusters[26] = 3;
    volume.clusters[28..32].copy_from_slice(&700u32.to_le_bytes());
    if let Some((&selector, bytes)) = data.split_first() {
        match selector % 4 {
            0 => replace(&mut volume.boot[11..], bytes),
            1 => replace(&mut volume.fat[8..], bytes),
            2 => replace(&mut volume.clusters, bytes),
            _ => replace(&mut volume.clusters[32..], bytes),
        }
    }

    let Ok(fs) = Fat32::mount(volume) else { return };
    if let Ok(mut file) = fs.open("/sample.bin") {
        let mut out = [0; 32];
        let offset = data.get(1).map_or(510, |byte| u64::from(*byte) * 4);
        let _ = file.read_at(offset, &mut out);
        let _ = file.read_at(0, &mut out);
    }
    if let Ok(directory) = fs.read_dir("/") {
        for entry in directory.take(16).map_while(Result::ok) {
            let name = format!("/{}", entry.name());
            if entry.kind == Kind::Directory {
                if let Ok(children) = fs.read_dir(&name) {
                    let _ = children.take(4).count();
                }
            } else if let Ok(mut file) = fs.open(&name) {
                let _ = file.read_at(0, &mut [0; 32]);
            }
        }
    }
});
