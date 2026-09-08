//! Literal, case-insensitive matching with ranges in the original UTF-8 text.
use std::ops::Range;

pub(crate) struct LiteralSearch {
    folded: String,
}

struct FoldedCharacter {
    folded: Range<usize>,
    original: Range<usize>,
}

impl LiteralSearch {
    pub fn new(query: &str) -> Self {
        Self {
            folded: query.to_lowercase(),
        }
    }

    pub fn ranges(&self, text: &str) -> Vec<Range<usize>> {
        if self.folded.is_empty() {
            return Vec::new();
        }
        if text.is_ascii() {
            return text
                .to_ascii_lowercase()
                .match_indices(&self.folded)
                .map(|(start, found)| start..start + found.len())
                .collect();
        }
        let mut folded = String::with_capacity(text.len());
        let mut characters = Vec::new();
        for (start, character) in text.char_indices() {
            let folded_start = folded.len();
            folded.extend(character.to_lowercase());
            characters.push(FoldedCharacter {
                folded: folded_start..folded.len(),
                original: start..start + character.len_utf8(),
            });
        }
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for (start, found) in folded.match_indices(&self.folded) {
            let end = start + found.len();
            let first = characters.partition_point(|character| character.folded.end <= start);
            let last = characters.partition_point(|character| character.folded.start < end);
            let original = characters[first].original.start..characters[last - 1].original.end;
            if let Some(previous) = ranges
                .last_mut()
                .filter(|previous| previous.end > original.start)
            {
                previous.end = previous.end.max(original.end);
            } else {
                ranges.push(original);
            }
        }
        ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn case_expansion_keeps_original_character_boundaries() {
        let text = "İ x İΧ";
        assert_eq!(LiteralSearch::new("i").ranges(text), vec![0..2, 5..7]);
        assert_eq!(LiteralSearch::new("χ").ranges(text), vec![7..9]);
    }
}
