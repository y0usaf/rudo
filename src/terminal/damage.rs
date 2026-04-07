//! Damage tracking - only re-render what changed.
//! Inspired by foot's per-row dirty tracking with bitset.

/// Tracks which rows need re-rendering.
#[derive(Clone, Debug)]
pub struct DamageTracker {
    dirty: Vec<u64>,
    num_rows: usize,
    full_damage: bool,
}

#[allow(dead_code)]
impl DamageTracker {
    pub fn new(rows: usize) -> Self {
        let words = (rows + 63) / 64;
        Self {
            dirty: vec![!0u64; words],
            num_rows: rows,
            full_damage: true,
        }
    }

    #[inline]
    pub fn mark_row(&mut self, row: usize) {
        if row < self.num_rows {
            self.dirty[row / 64] |= 1u64 << (row % 64);
        }
    }

    pub fn mark_all(&mut self) {
        self.full_damage = true;
    }

    #[inline]
    pub fn is_dirty(&self, row: usize) -> bool {
        self.full_damage
            || (row < self.num_rows && (self.dirty[row / 64] & (1u64 << (row % 64))) != 0)
    }

    pub fn has_damage(&self) -> bool {
        self.full_damage || self.dirty.iter().any(|&w| w != 0)
    }

    #[inline]
    pub fn is_full_damage(&self) -> bool {
        self.full_damage
    }

    pub fn dirty_row_ranges(&self) -> Vec<(usize, usize)> {
        if self.num_rows == 0 {
            return Vec::new();
        }
        if self.full_damage {
            return vec![(0, self.num_rows - 1)];
        }

        let mut ranges = Vec::new();
        let mut start = None;

        for row in 0..self.num_rows {
            if self.is_dirty(row) {
                if start.is_none() {
                    start = Some(row);
                }
            } else if let Some(range_start) = start.take() {
                ranges.push((range_start, row - 1));
            }
        }

        if let Some(range_start) = start {
            ranges.push((range_start, self.num_rows - 1));
        }

        ranges
    }

    pub fn clear(&mut self) {
        self.full_damage = false;
        self.dirty.fill(0);
    }

    pub fn resize(&mut self, rows: usize) {
        self.num_rows = rows;
        let words = (rows + 63) / 64;
        self.dirty.resize(words, !0u64);
        self.full_damage = true;
    }
}
