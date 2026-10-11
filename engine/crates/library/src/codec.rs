//! Codecs are stored in the DB as their wire names (protocol `Codec`, kebab-case).
use protocol::Codec;

pub fn name(c: Codec) -> &'static str {
    match c {
        Codec::Mp3 => "mp3", Codec::AacLc => "aac-lc", Codec::HeAac => "he-aac", Codec::HeAacV2 => "he-aac-v2",
        Codec::Alac => "alac", Codec::Flac => "flac", Codec::Pcm => "pcm", Codec::Vorbis => "vorbis",
        Codec::Opus => "opus", Codec::Dsd => "dsd",
    }
}

pub fn parse(s: &str) -> Option<Codec> {
    Some(match s {
        "mp3" => Codec::Mp3, "aac-lc" => Codec::AacLc, "he-aac" => Codec::HeAac, "he-aac-v2" => Codec::HeAacV2,
        "alac" => Codec::Alac, "flac" => Codec::Flac, "pcm" => Codec::Pcm, "vorbis" => Codec::Vorbis,
        "opus" => Codec::Opus, "dsd" => Codec::Dsd,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_match_the_wire_format() {
        for c in [protocol::Codec::Mp3, protocol::Codec::HeAacV2, protocol::Codec::Dsd] {
            assert_eq!(serde_json::to_value(c).unwrap(), super::name(c));
            assert_eq!(super::parse(super::name(c)), Some(c));
        }
    }
}
