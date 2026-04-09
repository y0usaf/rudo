//! Damage tracking - only re-render what changed.
//! Inspired by foot's per-row dirty tracking with bitset.

/// Tracks which rows need re-rendering.
#[derive(Clone, Debug)]
pub struct DamageTracker {
    dirty: Vec<u64>,
    num_rows: usize,
    full_damage: bool,
}

impl DamageTracker {
    pub fn new(rows: usize) -> Self {
        let words = rows.div_ceil(64);
        Self {
            dirty: vec![!0u64; words],
            num_rows: rows,
            full_damage: true,
        }
    }

    #[inline]
    fn row_bit(row: usize) -> (usize, u64) {
        (row / 64, 1u64 << (row % 64))
    }

    #[inline]
    fn valid_mask(valid_bits: usize) -> u64 {
        match valid_bits {
            0 | 64 => u64::MAX,
            bits => (1u64 << bits) - 1,
        }
    }

    #[inline]
    pub fn mark_row(&mut self, row: usize) {
        if self.full_damage || row >= self.num_rows {
            return;
        }

        let (word, bit) = Self::row_bit(row);
        self.dirty[word] |= bit;
    }

    #[inline]
    pub fn mark_rows(&mut self, start: usize, end: usize) {
        if self.full_damage || self.num_rows == 0 {
            return;
        }

        let start = start.min(self.num_rows - 1);
        let end = end.min(self.num_rows - 1);
        if start > end {
            return;
        }

        let start_word = start / 64;
        let end_word = end / 64;
        let start_bit = start % 64;
        let end_bit = end % 64;

        if start_word == end_word {
            let high = u64::MAX >> (63 - end_bit);
            let low = u64::MAX << start_bit;
            self.dirty[start_word] |= high & low;
            return;
        }

        self.dirty[start_word] |= u64::MAX << start_bit;
        if end_word > start_word + 1 {
            self.dirty[(start_word + 1)..end_word].fill(u64::MAX);
        }
        self.dirty[end_word] |= u64::MAX >> (63 - end_bit);
    }

    pub fn mark_all(&mut self) {
        self.full_damage = true;
    }

    #[inline]
    pub fn mark_scroll(&mut self, top: usize, bottom: usize, _lines: usize) {
        if self.full_damage || self.num_rows == 0 || top > bottom {
            return;
        }

        self.mark_rows(top, bottom);
    }

    #[inline]
    pub fn is_full_damage(&self) -> bool {
        self.full_damage
    }

    #[inline]
    pub fn has_damage(&self) -> bool {
        self.full_damage || self.dirty.iter().any(|&word| word != 0)
    }

    pub fn for_each_dirty_row_range(&self, mut f: impl FnMut(usize, usize)) {
        if self.num_rows == 0 {
            return;
        }
        if self.full_damage {
            f(0, self.num_rows - 1);
            return;
        }

        let mut current_start = None;
        let mut current_end = 0usize;

        for (word_idx, &word) in self.dirty.iter().enumerate() {
            let base_row = word_idx * 64;
            if base_row >= self.num_rows {
                break;
            }

            let mut bits = word & Self::valid_mask((self.num_rows - base_row).min(64));
            while bits != 0 {
                let row = base_row + bits.trailing_zeros() as usize;
                if let Some(start) = current_start {
                    if row == current_end.saturating_add(1) {
                        current_end = row;
                    } else {
                        f(start, current_end);
                        current_start = Some(row);
                        current_end = row;
                    }
                } else {
                    current_start = Some(row);
                    current_end = row;
                }
                bits &= bits - 1;
            }
        }

        if let Some(start) = current_start {
            f(start, current_end);
        }
    }

    pub fn dirty_row_ranges_into(&self, ranges: &mut Vec<(usize, usize)>) {
        ranges.clear();
        self.for_each_dirty_row_range(|start, end| ranges.push((start, end)));
    }

    #[cfg(test)]
    pub fn dirty_row_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        self.dirty_row_ranges_into(&mut ranges);
        ranges
    }

    pub fn clear(&mut self) {
        self.full_damage = false;
        self.dirty.fill(0);
    }

    pub fn resize(&mut self, rows: usize) {
        self.num_rows = rows;
        let words = rows.div_ceil(64);
        self.dirty.resize(words, 0);
        self.dirty.fill(0);
        self.full_damage = true;
    }
}

#[cfg(test)]
mod tests {
    use super::DamageTracker;

    #[test]
    fn mark_rows_handles_single_and_multi_word_ranges() {
        let mut damage = DamageTracker::new(130);
        damage.clear();

        damage.mark_rows(5, 5);
        damage.mark_rows(62, 66);
        damage.mark_rows(120, 200);

        assert_eq!(
            damage.dirty_row_ranges(),
            vec![(5, 5), (62, 66), (120, 129)]
        );
    }

    #[test]
    fn mark_rows_ignores_invalid_ranges() {
        let mut damage = DamageTracker::new(16);
        damage.clear();
        damage.mark_rows(10, 3);

        assert!(damage.dirty_row_ranges().is_empty());
    }

    #[test]
    fn dirty_row_ranges_merges_across_word_boundaries() {
        let mut damage = DamageTracker::new(130);
        damage.clear();
        damage.mark_rows(60, 70);
        damage.mark_rows(71, 80);

        assert_eq!(damage.dirty_row_ranges(), vec![(60, 80)]);
    }

    #[test]
    fn dirty_row_ranges_handles_exact_word_boundaries() {
        let mut damage = DamageTracker::new(130);
        damage.clear();
        damage.mark_rows(63, 64);

        assert_eq!(damage.dirty_row_ranges(), vec![(63, 64)]);
    }

    #[test]
    fn dirty_row_ranges_clamps_trailing_bits_to_visible_rows() {
        let mut damage = DamageTracker::new(65);
        damage.clear();
        damage.mark_rows(64, 128);

        assert_eq!(damage.dirty_row_ranges(), vec![(64, 64)]);
    }

    #[test]
    fn mark_row_and_mark_rows_are_noops_under_full_damage() {
        let mut damage = DamageTracker::new(8);
        let before = damage.dirty_row_ranges();

        damage.mark_row(3);
        damage.mark_rows(1, 4);

        assert!(damage.is_full_damage());
        assert_eq!(damage.dirty_row_ranges(), before);
    }
}
