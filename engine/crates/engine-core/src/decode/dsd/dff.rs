use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::{validate_format, DsdSource};
use crate::decode::DecodeError;

pub struct DffReader { r: BufReader<File>, channels: u16, rate: u32, data_start: u64, total_bytes: u64, pos: u64, buf: Vec<u8> }

fn hdr<R: Read>(r: &mut R) -> io::Result<([u8; 4], u64)> {
    let mut b = [0u8; 12];
    r.read_exact(&mut b)?;
    Ok((b[0..4].try_into().unwrap(), u64::from_be_bytes(b[4..12].try_into().unwrap())))
}

pub fn open(path: &Path) -> Result<Box<dyn DsdSource>, DecodeError> {
    let bad = |m: &str| DecodeError::Unsupported(format!("DFF: {m}"));
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut r = BufReader::new(file);
    let (id, _) = hdr(&mut r)?;
    let mut form = [0u8; 4];
    r.read_exact(&mut form)?;
    if &id != b"FRM8" || &form != b"DSD " { return Err(bad("not a DSDIFF file")); }
    let (mut channels, mut rate, mut data) = (0u64, 0u64, None);
    while let Ok((id, size)) = hdr(&mut r) {
        let start = r.stream_position()?;
        let end = start.checked_add(size).ok_or_else(|| bad("chunk size overflow"))?;
        match &id {
            b"PROP" => {
                if end > file_len { return Err(bad("PROP chunk exceeds file")); }
                let mut kind = [0u8; 4];
                r.read_exact(&mut kind)?;
                while r.stream_position()? + 12 <= end {
                    let (sid, ssize) = hdr(&mut r)?;
                    let sstart = r.stream_position()?;
                    let send = sstart.checked_add(ssize).filter(|&e| e <= end).ok_or_else(|| bad("sub-chunk exceeds PROP chunk"))?;
                    let mut body = vec![0u8; ssize as usize];
                    r.read_exact(&mut body)?;
                    let need = |n: usize| if body.len() >= n { Ok(()) } else { Err(bad("short sub-chunk")) };
                    match &sid {
                        b"FS  " => { need(4)?; rate = u32::from_be_bytes(body[..4].try_into().unwrap()).into(); }
                        b"CHNL" => { need(2)?; channels = u16::from_be_bytes(body[..2].try_into().unwrap()).into(); }
                        b"CMPR" => { need(4)?; if &body[..4] != b"DSD " { return Err(bad("DST-compressed DSD is not supported")); } }
                        _ => {}
                    }
                    r.seek(SeekFrom::Start(send + (ssize & 1)))?;
                }
            }
            b"DSD " => { data = Some((start, size.min(file_len.saturating_sub(start)))); break; }
            _ => {}
        }
        r.seek(SeekFrom::Start(end.saturating_add(size & 1)))?;
    }
    let (data_start, len) = data.ok_or_else(|| bad("no DSD data chunk"))?;
    let (channels, rate) = validate_format("DFF", channels, rate)?;
    Ok(Box::new(DffReader { r, channels, rate, data_start, total_bytes: len / channels as u64, pos: 0, buf: Vec::new() }))
}

impl DsdSource for DffReader {
    fn channels(&self) -> u16 { self.channels }
    fn dsd_rate(&self) -> u32 { self.rate }
    fn total_bytes_per_channel(&self) -> u64 { self.total_bytes }

    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> io::Result<usize> {
        out.iter_mut().for_each(Vec::clear);
        let n = (max as u64).min(self.total_bytes - self.pos) as usize;
        if n == 0 { return Ok(0); }
        let ch = self.channels as usize;
        self.r.seek(SeekFrom::Start(self.data_start + self.pos * ch as u64))?;
        self.buf.resize(n * ch, 0);
        self.r.read_exact(&mut self.buf)?;
        for (c, o) in out.iter_mut().enumerate() { o.extend(self.buf.iter().skip(c).step_by(ch)); }
        self.pos += n as u64;
        Ok(n)
    }

    fn seek_byte(&mut self, byte: u64) -> io::Result<()> { self.pos = byte.min(self.total_bytes); Ok(()) }
}
