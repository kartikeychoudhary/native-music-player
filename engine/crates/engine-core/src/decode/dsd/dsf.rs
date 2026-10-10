use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::DsdSource;
use crate::decode::DecodeError;

pub struct DsfReader {
    r: BufReader<File>,
    channels: u16,
    rate: u32,
    lsb_first: bool,
    block: usize,
    data_start: u64,
    total_bytes: u64, // per channel, valid bytes only
    pos: u64,         // per-channel byte position
    group: Vec<u8>,
}

fn u32le(b: &[u8]) -> u32 { u32::from_le_bytes(b[..4].try_into().unwrap()) }
fn u64le(b: &[u8]) -> u64 { u64::from_le_bytes(b[..8].try_into().unwrap()) }

pub fn open(path: &Path) -> Result<Box<dyn DsdSource>, DecodeError> {
    let mut r = BufReader::new(File::open(path)?);
    let mut hdr = [0u8; 28 + 52 + 12];
    r.read_exact(&mut hdr).map_err(|_| DecodeError::Unsupported("truncated DSF".into()))?;
    if &hdr[0..4] != b"DSD " || &hdr[28..32] != b"fmt " || &hdr[80..84] != b"data" {
        return Err(DecodeError::Unsupported("not a DSF file".into()));
    }
    let fmt = &hdr[40..80];
    let (format_id, channels, rate, bps) = (u32le(&fmt[4..]), u32le(&fmt[12..]) as u16, u32le(&fmt[16..]), u32le(&fmt[20..]));
    if format_id != 0 { return Err(DecodeError::Unsupported("DSF: compressed DSD".into())); }
    let sample_count = u64le(&fmt[24..]);
    let block = u32le(&fmt[32..]) as usize;
    if channels == 0 || block == 0 { return Err(DecodeError::Corrupt("DSF: bad fmt chunk".into())); }
    Ok(Box::new(DsfReader {
        r, channels, rate, lsb_first: bps == 1, block,
        data_start: 92, total_bytes: sample_count / 8, pos: 0, group: vec![0; block * channels as usize],
    }))
}

impl DsdSource for DsfReader {
    fn channels(&self) -> u16 { self.channels }
    fn dsd_rate(&self) -> u32 { self.rate }
    fn total_bytes_per_channel(&self) -> u64 { self.total_bytes }

    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> io::Result<usize> {
        out.iter_mut().for_each(Vec::clear);
        if self.pos >= self.total_bytes { return Ok(0); }
        let block_idx = self.pos / self.block as u64;
        let in_block = (self.pos % self.block as u64) as usize;
        self.r.seek(SeekFrom::Start(self.data_start + block_idx * (self.block * self.channels as usize) as u64))?;
        self.r.read_exact(&mut self.group)?;
        let n = (self.block - in_block).min(max).min((self.total_bytes - self.pos) as usize);
        for (ch, o) in out.iter_mut().enumerate() {
            let s = ch * self.block + in_block;
            o.extend(self.group[s..s + n].iter().map(|&b| if self.lsb_first { b.reverse_bits() } else { b }));
        }
        self.pos += n as u64;
        Ok(n)
    }

    fn seek_byte(&mut self, byte: u64) -> io::Result<()> { self.pos = byte.min(self.total_bytes); Ok(()) }
}
