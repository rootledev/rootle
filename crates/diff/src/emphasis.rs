use crate::{ChangedSpan, Hunk, LineOrigin};

impl Hunk {
    /// Pair adjacent deletion/addition runs once. Each span belongs to its
    /// own line, including unequal replacement lengths and multibyte text.
    pub fn changed_spans(&self) -> Vec<Option<ChangedSpan>> {
        let mut spans = vec![None; self.lines.len()];
        let mut position = 0;
        while position < self.lines.len() {
            if self.lines[position].origin != LineOrigin::Deletion {
                position += 1;
                continue;
            }
            let deleted_start = position;
            while position < self.lines.len() && self.lines[position].origin == LineOrigin::Deletion
            {
                position += 1;
            }
            let added_start = position;
            while position < self.lines.len() && self.lines[position].origin == LineOrigin::Addition
            {
                position += 1;
            }
            let pairs = (added_start - deleted_start).min(position - added_start);
            for offset in 0..pairs {
                let old_index = deleted_start + offset;
                let new_index = added_start + offset;
                let (old, new) =
                    changed_pair(&self.lines[old_index].text, &self.lines[new_index].text);
                spans[old_index] = (!old.is_empty()).then_some(ChangedSpan(old));
                spans[new_index] = (!new.is_empty()).then_some(ChangedSpan(new));
            }
        }
        spans
    }
}

fn changed_pair(old: &str, new: &str) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(character, _)| character.len_utf8())
        .sum();
    // Restrict the suffix to what remains after the shared prefix: it
    // cannot overlap it, including identical and all-prefix replacements.
    let suffix: usize = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(character, _)| character.len_utf8())
        .sum();
    (prefix..old.len() - suffix, prefix..new.len() - suffix)
}
