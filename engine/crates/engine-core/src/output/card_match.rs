//! Pure ALSA card-name matching, kept target-independent so it is unit-tested on every OS.

/// Index of the ALSA card matching a cpal/PipeWire device `name`. `cards` holds
/// `(index, short name, long name)`. Matches by case-insensitive substring in either direction:
/// the short name inside `name`, or the long name (minus the trailing " at <bus address>")
/// inside `name` or containing it. Empty card names never match.
pub fn card_for(name: &str, cards: &[(i32, String, String)]) -> Option<i32> {
    let n = name.to_ascii_lowercase();
    cards.iter().find(|(_, short, long)| {
        let s = short.to_ascii_lowercase();
        let l = long.to_ascii_lowercase();
        let l_head = l.split(" at ").next().unwrap_or(&l);
        (!s.is_empty() && n.contains(&s)) || (!l_head.is_empty() && (n.contains(l_head) || l_head.contains(&n)))
    }).map(|(i, _, _)| *i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards() -> Vec<(i32, String, String)> {
        vec![(0, "PCH".into(), "HDA Intel PCH".into()), (2, "D50s".into(), "Topping D50s at usb-0000:00:14.0-2".into())]
    }

    #[test]
    fn matches_cards_by_name() {
        let cards = cards();
        assert_eq!(card_for("Topping D50s Analog Stereo", &cards), Some(2));
        assert_eq!(card_for("Built-in Audio (HDA Intel PCH)", &cards), Some(0));
        assert_eq!(card_for("Something else", &cards), None);
    }

    #[test]
    fn empty_card_names_never_match() {
        let cards = vec![(1, String::new(), String::new())];
        assert_eq!(card_for("Anything", &cards), None);
    }
}
