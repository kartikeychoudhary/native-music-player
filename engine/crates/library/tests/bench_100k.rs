//! Spec §1.4: first scan of 100k tracks < 2 min on an SSD; paging/search queries < 50 ms.
//! Run: cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture
//! AURORA_BENCH_TRACKS overrides the corpus size (default 100000).
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use library::Library;
use protocol::{ListFilter, ListQuery, SortKey};
use rayon::prelude::*;

const SYLLABLES: [&str; 20] = ["ka", "ve", "lo", "mi", "ru", "sa", "te", "no", "pi", "da", "ze", "fu", "ri", "go", "ha", "ne", "so", "bi", "ta", "ly"];

/// A deterministic pronounceable word for `n` (distinct for distinct `n`).
fn word(mut n: usize) -> String {
    let mut w = String::new();
    loop {
        w.push_str(SYLLABLES[n % 20]);
        n /= 20;
        if n == 0 { break; }
    }
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn info_chunk(id: &[u8; 4], value: &str) -> Vec<u8> {
    let mut data = value.as_bytes().to_vec();
    data.push(0);
    let mut out = id.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    if data.len() % 2 == 1 { data.push(0); }
    out.extend(data);
    out
}

/// 10 ms of 44.1 kHz/16-bit stereo silence with a RIFF INFO tag (INAM/IART/IPRD/IGNR/ITRK).
fn tiny_wav(title: &str, artist: &str, album: &str, genre: &str, track: usize) -> Vec<u8> {
    let mut list = b"INFO".to_vec();
    for (id, v) in [(b"INAM", title), (b"IART", artist), (b"IPRD", album), (b"IGNR", genre)] { list.extend(info_chunk(id, v)); }
    list.extend(info_chunk(b"ITRK", &track.to_string()));
    let pcm = vec![0u8; 441 * 4];
    let mut body = b"WAVE".to_vec();
    body.extend(b"fmt ");
    body.extend(16u32.to_le_bytes());
    body.extend(1u16.to_le_bytes()); // PCM
    body.extend(2u16.to_le_bytes()); // channels
    body.extend(44100u32.to_le_bytes());
    body.extend((44100u32 * 4).to_le_bytes());
    body.extend(4u16.to_le_bytes());
    body.extend(16u16.to_le_bytes());
    body.extend(b"LIST");
    body.extend((list.len() as u32).to_le_bytes());
    body.extend(list);
    body.extend(b"data");
    body.extend((pcm.len() as u32).to_le_bytes());
    body.extend(pcm);
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A named query to time.
type Case<'a> = (&'a str, Box<dyn Fn() + 'a>);

const PER_ALBUM: usize = 10;
const ALBUMS_PER_ARTIST: usize = 5;
const GENRES: [&str; 12] = ["Ambient", "Jazz", "Rock", "Folk", "Soul", "Funk", "House", "Techno", "Classical", "Blues", "Pop", "Metal"];

fn artist_name(a: usize) -> String { format!("{} {}", word(a + 400), word(a * 7 + 13)) }

fn generate(root: &Path, n: usize) {
    let _ = std::fs::remove_dir_all(root);
    let albums = n.div_ceil(PER_ALBUM);
    (0..albums).into_par_iter().for_each(|al| {
        let a = al / ALBUMS_PER_ARTIST;
        let artist = artist_name(a);
        let album = format!("{} {}", word(al * 3 + 1), word(al + 9000));
        let dir = root.join(&artist).join(&album);
        std::fs::create_dir_all(&dir).unwrap();
        for t in 0..PER_ALBUM {
            let i = al * PER_ALBUM + t;
            if i >= n { break; }
            let title = format!("{} {}", word(i * 31 + 5), word(i + 77));
            std::fs::write(dir.join(format!("{:02}.wav", t + 1)), tiny_wav(&title, &artist, &album, GENRES[al % GENRES.len()], t + 1)).unwrap();
        }
    });
    std::fs::write(root.join(".complete"), b"").unwrap();
}

fn median(mut f: impl FnMut()) -> Duration {
    f(); // warm the page cache and statement cache
    let mut times: Vec<Duration> = (0..5).map(|_| { let t = Instant::now(); f(); t.elapsed() }).collect();
    times.sort();
    times[2]
}

#[test]
#[ignore = "benchmark; run with --release --ignored"]
fn library_100k_meets_targets() {
    let n: usize = std::env::var("AURORA_BENCH_TRACKS").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let corpus = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bench-corpus-{n}"));
    if !corpus.join(".complete").exists() {
        let t = Instant::now();
        generate(&corpus, n);
        println!("generated {n} files in {:.1} s", t.elapsed().as_secs_f64());
    }
    let data = tempfile::tempdir().unwrap();
    let (lib, _) = Library::open(data.path()).unwrap();
    let folder = lib.add_folder(&corpus).unwrap().id;

    let t = Instant::now();
    let summary = lib.scanner(Default::default()).unwrap().scan(folder, &[], true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let scan = t.elapsed();
    println!("first scan: {} tracks in {:.1} s", summary.added, scan.as_secs_f64());
    assert_eq!(summary.added as usize, n);
    assert!(scan < Duration::from_secs(120), "first scan took {scan:?} (target < 120 s)");

    let t = Instant::now();
    let again = lib.scanner(Default::default()).unwrap().scan(folder, &[], true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    println!("rescan (nothing changed): {:.1} s", t.elapsed().as_secs_f64());
    assert_eq!(again.unchanged as usize, n);

    let some_album = lib.albums(&ListQuery { offset: Some(1234), limit: Some(1), ..Default::default() }).unwrap().items[0].id;
    let some_artist = lib.artists(&ListQuery { offset: Some(321), limit: Some(1), ..Default::default() }).unwrap().items[0].id;
    let artist_prefix: String = artist_name(4217 % (n / PER_ALBUM / ALBUMS_PER_ARTIST).max(1)).chars().take(5).collect();
    let page = |sort, desc, offset| ListQuery { sort: Some(sort), desc: Some(desc), offset: Some(offset), limit: Some(100), filter: None };

    let cases: Vec<Case> = vec![
        ("albums by title", Box::new(|| { lib.albums(&page(SortKey::Title, false, 0)).unwrap(); })),
        ("albums by artist, deep page", Box::new(|| { lib.albums(&page(SortKey::Artist, false, 4000)).unwrap(); })),
        ("albums by year desc", Box::new(|| { lib.albums(&page(SortKey::Year, true, 0)).unwrap(); })),
        ("albums recently added", Box::new(|| { lib.albums(&page(SortKey::AddedAt, true, 0)).unwrap(); })),
        ("tracks by title, offset 50k", Box::new(|| { lib.tracks(&page(SortKey::Title, false, (n / 2) as u32)).unwrap(); })),
        ("tracks by artist, offset 50k", Box::new(|| { lib.tracks(&page(SortKey::Artist, false, (n / 2) as u32)).unwrap(); })),
        ("tracks of one artist", Box::new(|| {
            lib.tracks(&ListQuery { filter: Some(ListFilter { artist_id: Some(some_artist), ..Default::default() }), ..Default::default() }).unwrap();
        })),
        ("albums of one artist", Box::new(|| {
            lib.albums(&ListQuery { filter: Some(ListFilter { artist_id: Some(some_artist), ..Default::default() }), ..Default::default() }).unwrap();
        })),
        ("artists page", Box::new(|| { lib.artists(&page(SortKey::Name, false, 500)).unwrap(); })),
        ("search artist prefix", Box::new(|| { lib.search(&artist_prefix, 20).unwrap(); })),
        ("search broad 2-char prefix", Box::new(|| { lib.search("ka", 20).unwrap(); })),
        ("search two words", Box::new(|| { lib.search("kave lo", 20).unwrap(); })),
        ("album detail", Box::new(|| { lib.album(some_album).unwrap(); })),
        ("artist detail", Box::new(|| { lib.artist(some_artist).unwrap(); })),
    ];
    let mut failures = vec![];
    for (name, f) in &cases {
        let m = median(f);
        println!("{name:<32} {:>7.2} ms", m.as_secs_f64() * 1000.0);
        if m >= Duration::from_millis(50) { failures.push(format!("{name}: {m:?}")); }
    }
    assert!(failures.is_empty(), "queries over 50 ms: {failures:?}");
}
