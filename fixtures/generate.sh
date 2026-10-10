#!/usr/bin/env bash
# Generates synthetic test audio into fixtures/out (git-ignored). Needs ffmpeg and cargo.
set -euo pipefail
cd "$(dirname "$0")"
OUT=out
mkdir -p "$OUT"
FF="ffmpeg -hide_banner -loglevel error -y"

# Reference signals: L = 440 Hz, R = 1 kHz, sine amplitude 1/8, 2 s.
tone() { # rate dur out
  $FF -f lavfi -i "sine=f=440:d=$2:sample_rate=$1" -f lavfi -i "sine=f=1000:d=$2:sample_rate=$1" \
      -filter_complex "[0][1]amerge=inputs=2" "${@:3}"
}
tone 44100 2 -c:a pcm_s16le "$OUT/pcm16_44k.wav"
tone 96000 2 -c:a pcm_s24le "$OUT/pcm24_96k.wav"

# Lossless encodes + raw left-justified s32 references.
for src in pcm16_44k pcm24_96k; do
  $FF -i "$OUT/$src.wav" -c:a flac "$OUT/${src/pcm/flac}.flac"
  $FF -i "$OUT/$src.wav" -c:a alac "$OUT/${src/pcm/alac}.m4a"
  $FF -i "$OUT/$src.wav" -f s32le "$OUT/$src.s32le"
done
$FF -i "$OUT/pcm16_44k.wav" -c:a pcm_s16be "$OUT/aiff16_44k.aiff"
$FF -i "$OUT/pcm16_44k.wav" -f f32le "$OUT/pcm16_44k.f32le"

# Lossy encodes.
$FF -i "$OUT/pcm16_44k.wav" -c:a libmp3lame -b:a 320k "$OUT/mp3_320.mp3"
$FF -i "$OUT/pcm16_44k.wav" -c:a libmp3lame -q:a 0 "$OUT/mp3_v0.mp3"
$FF -i "$OUT/pcm16_44k.wav" -c:a aac -b:a 256k "$OUT/aac_lc_256.m4a"
$FF -i "$OUT/pcm16_44k.wav" -c:a libvorbis -q:a 6 "$OUT/vorbis_q6.ogg"
$FF -i "$OUT/pcm16_44k.wav" -c:a libopus -b:a 160k "$OUT/opus_160.opus"

# Gapless pairs: 6 s continuous tone split at a non-frame-aligned sample (110583).
tone 44100 6 -c:a pcm_s16le "$OUT/gap_src.wav"
$FF -i "$OUT/gap_src.wav" -f f32le "$OUT/gap_src.f32le"
$FF -i "$OUT/gap_src.wav" -af "atrim=end_sample=110583" "$OUT/gap_a.wav"
$FF -i "$OUT/gap_src.wav" -af "atrim=start_sample=110583,asetpts=PTS-STARTPTS" "$OUT/gap_b.wav"
for p in a b; do
  $FF -i "$OUT/gap_$p.wav" -c:a flac "$OUT/gap_$p.flac"
  $FF -i "$OUT/gap_$p.wav" -c:a libmp3lame -b:a 320k "$OUT/gap_$p.mp3"
  $FF -i "$OUT/gap_$p.wav" -c:a aac -b:a 256k "$OUT/gap_$p.m4a"
  $FF -i "$OUT/gap_$p.wav" -c:a libvorbis -q:a 6 "$OUT/gap_$p.ogg"
  $FF -i "$OUT/gap_$p.wav" -c:a libopus -b:a 160k "$OUT/gap_$p.opus"
done

# Channel layouts, short and empty files.
$FF -f lavfi -i "sine=f=440:d=1:sample_rate=44100" -c:a flac "$OUT/mono16_44k.flac"
$FF -f lavfi -i "sine=f=440:d=1:sample_rate=48000" -af "pan=5.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0" -c:a flac "$OUT/surround51_48k.flac"
$FF -f lavfi -i "sine=f=440:d=0.5:sample_rate=44100" -ac 2 -c:a flac "$OUT/short_a.flac"
$FF -f lavfi -i "sine=f=440:d=0.5:sample_rate=44100" -ac 2 -c:a flac "$OUT/short_b.flac"
: > "$OUT/empty.mp3"

# HE-AAC, DSD and corrupt MP3 from fixtures-gen, then remux HE-AAC into M4A.
cargo run --quiet --manifest-path ../engine/Cargo.toml -p fixtures-gen -- "$PWD/$OUT"
$FF -i "$OUT/he_aac.aac" -c copy "$OUT/he_aac.m4a"
$FF -i "$OUT/he_aac_v2.aac" -c copy "$OUT/he_aac_v2.m4a"
echo "fixtures: $(ls "$OUT" | wc -l) files in fixtures/$OUT"
