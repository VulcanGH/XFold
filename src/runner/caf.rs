use super::{Result, check_cancel};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

/// Reserve 12 dB so Harlettizer can store the compatible speaker mixes.
/// Edit the scratch master in place and keep its sample rate and frame count.
pub(super) fn reserve_headroom(path: &Path, cancel: &AtomicBool) -> Result<u64> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let invalid = "TrueHD preparation expected a 48 kHz, 24-bit PCM CAF master.";
    let file_size = file.metadata()?.len();
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    if header[..6] != *b"caff\0\x01" {
        return Err(invalid.into());
    }
    let mut frame_width = None;
    let (start, length, width) = loop {
        let mut chunk = [0; 12];
        file.read_exact(&mut chunk)?;
        let size = u64::from_be_bytes(chunk[4..].try_into()?);
        let pos = file.stream_position()?;
        let end = pos
            .checked_add(size)
            .filter(|&end| end <= file_size)
            .ok_or(invalid)?;
        match &chunk[..4] {
            b"desc" if size == 32 => {
                let mut desc = [0; 32];
                file.read_exact(&mut desc)?;
                let rate = f64::from_be_bytes(desc[..8].try_into()?);
                let field = |i| u32::from_be_bytes(desc[i..i + 4].try_into().unwrap());
                let channels = field(24);
                if rate != 48_000.0
                    || desc[8..12] != *b"lpcm"
                    || field(12) != 0
                    || field(20) != 1
                    || !(1..=256).contains(&channels)
                    || field(28) != 24
                    || field(16) != channels * 3
                {
                    return Err(invalid.into());
                }
                frame_width = Some(u64::from(channels * 3));
            }
            b"data" => {
                let width = frame_width.ok_or(invalid)?;
                let length = size
                    .checked_sub(4)
                    .filter(|&n| n > 0 && n % width == 0)
                    .ok_or(invalid)?;
                break (pos + 4, length, width);
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(end))?;
    };

    let mut buf = vec![0; 3 * 1024 * 1024];
    let mut position = start;
    let end = start + length;
    while position < end {
        check_cancel(cancel)?;
        let count = (end - position).min(buf.len() as u64) as usize;
        let data = &mut buf[..count];
        file.seek(SeekFrom::Start(position))?;
        file.read_exact(data)?;
        for sample in data.chunks_exact_mut(3) {
            let value = i32::from_be_bytes([sample[0], sample[1], sample[2], 0]) >> 8;
            sample.copy_from_slice(&(value / 4).to_be_bytes()[1..]);
        }
        file.seek(SeekFrom::Start(position))?;
        file.write_all(data)?;
        position += count as u64;
    }
    Ok(length / width)
}
