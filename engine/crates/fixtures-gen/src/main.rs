//! Generates fixtures ffmpeg can't produce: HE-AAC (fdk), DSF/DFF and a corrupt MP3.
use anyhow::{Context, Result};
use engine_core::decode::dsd::modulator::sdm2;
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};
use std::{f64::consts::PI, fs, path::Path};

fn main() -> Result<()> {
    let out = std::env::args().nth(1).context("usage: fixtures-gen <out-dir>")?;
    let out = Path::new(&out);
    fs::create_dir_all(out)?;
    he_aac(out, "he_aac.aac", AudioObjectType::Mpeg4HeAac, 64_000)?;
    he_aac(out, "he_aac_v2.aac", AudioObjectType::Mpeg4HeAacV2, 32_000)?;
    for (name, mult) in [("dsd64", 64u32), ("dsd128", 128)] {
        let rate = 44_100 * mult;
        let (l, r) = dsd_tones(rate, 1.5);
        fs::write(out.join(format!("{name}.dsf")), dsf(rate, &l, &r))?;
    }
    let (l, r) = dsd_tones(44_100 * 64, 1.5);
    fs::write(out.join("dsd64.dff"), dff(44_100 * 64, &l, &r))?;
    corrupt_mp3(out)?;
    println!("fixtures-gen: done");
    Ok(())
}

/// 2 s stereo: 1 kHz left, 440 Hz right at -6 dBFS, 48 kHz.
fn he_aac(out: &Path, name: &str, aot: AudioObjectType, bitrate: u32) -> Result<()> {
    let enc = Encoder::new(EncoderParams {
        bit_rate: BitRate::Cbr(bitrate),
        sample_rate: 48_000,
        transport: Transport::Adts,
        channels: ChannelMode::Stereo,
        audio_object_type: aot,
    })
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let frames = 96_000usize;
    let mut pcm = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let t = n as f64 / 48_000.0;
        pcm.push((0.5 * (2.0 * PI * 1000.0 * t).sin() * 32767.0) as i16);
        pcm.push((0.5 * (2.0 * PI * 440.0 * t).sin() * 32767.0) as i16);
    }
    // Pad with silence so the encoder flushes its look-ahead.
    pcm.extend(std::iter::repeat_n(0i16, 48_000));
    let mut bytes = Vec::new();
    let mut buf = vec![0u8; 8192];
    let mut pos = 0;
    while pos < pcm.len() {
        let end = (pos + 2048).min(pcm.len());
        let info = enc.encode(&pcm[pos..end], &mut buf).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        bytes.extend_from_slice(&buf[..info.output_size]);
        pos += info.input_consumed.max(1);
    }
    fs::write(out.join(name), bytes)?;
    Ok(())
}

/// Left 1 kHz, right 3 kHz, modulation amplitude 0.5 (-6 dB DSD).
fn dsd_tones(rate: u32, secs: f64) -> (Vec<u8>, Vec<u8>) {
    let n = (rate as f64 * secs) as usize / 8 * 8;
    let tone = |f: f64| (0..n).map(|i| 0.5 * (2.0 * PI * f * i as f64 / rate as f64).sin()).collect::<Vec<_>>();
    (sdm2(&tone(1000.0)), sdm2(&tone(3000.0)))
}

/// DSF: per-channel 4096-byte blocks, LSB-first bit order (bits_per_sample = 1).
fn dsf(rate: u32, l: &[u8], r: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 4096;
    let rev = |v: &[u8]| v.iter().map(|b| b.reverse_bits()).collect::<Vec<u8>>();
    let (l, r) = (rev(l), rev(r));
    let blocks = l.len().div_ceil(BLOCK);
    let mut data = Vec::with_capacity(blocks * BLOCK * 2);
    for b in 0..blocks {
        for ch in [&l, &r] {
            let s = b * BLOCK;
            let e = (s + BLOCK).min(ch.len());
            data.extend_from_slice(&ch[s..e]);
            data.extend(std::iter::repeat_n(0u8, BLOCK - (e - s)));
        }
    }
    let sample_count = (l.len() * 8) as u64;
    let mut f = Vec::new();
    let total = 28 + 52 + 12 + data.len() as u64;
    f.extend_from_slice(b"DSD ");
    f.extend_from_slice(&28u64.to_le_bytes());
    f.extend_from_slice(&total.to_le_bytes());
    f.extend_from_slice(&0u64.to_le_bytes()); // no metadata
    f.extend_from_slice(b"fmt ");
    f.extend_from_slice(&52u64.to_le_bytes());
    for v in [1u32, 0, 2, 2, rate, 1] { f.extend_from_slice(&v.to_le_bytes()); } // version, id, ch type, ch num, fs, bps
    f.extend_from_slice(&sample_count.to_le_bytes());
    f.extend_from_slice(&(BLOCK as u32).to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&(12 + data.len() as u64).to_le_bytes());
    f.extend_from_slice(&data);
    f
}

/// DSDIFF: big-endian, byte-interleaved channels, MSB-first.
fn dff(rate: u32, l: &[u8], r: &[u8]) -> Vec<u8> {
    let mut snd = Vec::new();
    let chunk = |id: &[u8; 4], body: &[u8]| {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u64).to_be_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 { c.push(0); }
        c
    };
    snd.extend_from_slice(b"SND ");
    snd.extend(chunk(b"FS  ", &rate.to_be_bytes()));
    let mut chnl = 2u16.to_be_bytes().to_vec();
    chnl.extend_from_slice(b"SLFTSRGT");
    snd.extend(chunk(b"CHNL", &chnl));
    let mut cmpr = b"DSD ".to_vec();
    cmpr.push(14);
    cmpr.extend_from_slice(b"not compressed");
    snd.extend(chunk(b"CMPR", &cmpr));
    let mut data = Vec::with_capacity(l.len() * 2);
    for i in 0..l.len() { data.push(l[i]); data.push(r[i]); }
    let mut body = b"DSD ".to_vec();
    body.extend(chunk(b"FVER", &0x0105_0000u32.to_be_bytes()));
    body.extend(chunk(b"PROP", &snd));
    body.extend(chunk(b"DSD ", &data));
    chunk(b"FRM8", &body)
}

fn corrupt_mp3(out: &Path) -> Result<()> {
    let src = out.join("mp3_320.mp3");
    let mut bytes = fs::read(&src).context("run the ffmpeg part of generate.sh first")?;
    let mid = bytes.len() / 2;
    for b in &mut bytes[mid..mid + 2048] { *b = 0x55; }
    fs::write(out.join("mp3_corrupt.mp3"), bytes)?;
    Ok(())
}
