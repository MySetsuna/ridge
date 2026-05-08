//! Fixed-capacity ring buffer for scrolled-off rows.
//!
//! When a line scrolls off the top of the visible grid (because of `\n` at
//! the bottom row, `IND`, or explicit `SU`), it goes here. The newest entry
//! is at index `len - 1`; once `capacity` is reached, pushes overwrite the
//! oldest entry and the read offset advances.
//!
//! VecDeque would also work, but a hand-rolled ring lets us reuse Row
//! allocations: when an old row is evicted we can hand its `Vec<Cell>`
//! back to the caller for reuse, avoiding allocator churn during heavy
//! output bursts.

use super::cell::Row;

pub struct Scrollback {
    /// Sparse storage. `entries[head..head+len]` (mod capacity) holds the
    /// actual rows in oldest-to-newest order.
    entries: Vec<Option<Row>>,
    /// Index of the oldest entry.
    head: usize,
    /// Number of valid entries currently stored.
    len: usize,
    capacity: usize,
    /// §B.2 (2026-05-08) — count of rows ever evicted from the OLDEST
    /// end via `push` at capacity. Selection / search abs-row anchors
    /// are stored as offsets relative to the scrollback front; when
    /// this counter advances, every previously-recorded abs_row N now
    /// points to what was abs_row N+evictions at recording time, so
    /// the JS-facing `feed` path uses the delta to decide whether to
    /// invalidate selection. This replaces the prior "clear on every
    /// feed" sledgehammer that killed TUI users' selections on every
    /// frame redraw — even when nothing actually scrolled.
    ///
    /// Wraps at u64::MAX (impossibly large session). `clear()` does
    /// NOT reset it — physical clear by ED 3 is itself a stronger
    /// signal than per-row eviction, and the JS layer treats both as
    /// "drop selection" via the same delta-comparison path.
    eviction_count: u64,
}

impl Scrollback {
    pub fn new(capacity: usize) -> Self {
        let mut entries = Vec::with_capacity(capacity);
        entries.resize_with(capacity, || None);
        Self {
            entries,
            head: 0,
            len: 0,
            capacity,
            eviction_count: 0,
        }
    }

    /// §B.2 — monotonically-increasing counter of evictions from the
    /// oldest end. Advances on each `push` call that has to overwrite
    /// the head slot (i.e. when the ring is at capacity). Used by the
    /// JS-facing `feed` path to detect "did this byte stream cause a
    /// row to roll off the top?" without inspecting individual cells.
    pub fn eviction_count(&self) -> u64 {
        self.eviction_count
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Push a row to the newest end. If at capacity, returns the evicted
    /// oldest row so the caller can recycle its allocation.
    pub fn push(&mut self, row: Row) -> Option<Row> {
        if self.capacity == 0 {
            // Degenerate: scrollback disabled.
            return Some(row);
        }
        if self.len < self.capacity {
            let idx = (self.head + self.len) % self.capacity;
            self.entries[idx] = Some(row);
            self.len += 1;
            None
        } else {
            // Evict head, advance, write new.
            let evicted = self.entries[self.head].take();
            self.entries[self.head] = Some(row);
            self.head = (self.head + 1) % self.capacity;
            self.eviction_count = self.eviction_count.wrapping_add(1);
            evicted
        }
    }

    /// Push a row to the OLDEST end. Used by `Terminal::prepend_scrollback`
    /// so backend-supplied history fetched via `get_pane_scrollback_before`
    /// can be inserted "above" the existing kernel scrollback when the user
    /// pages up past the wasm buffer boundary.
    ///
    /// If at capacity, evicts the **newest** row (the row directly above the
    /// live grid in the viewport). This is the least-bad trade-off: the user
    /// is actively browsing deep history, and we preserve the older rows
    /// they explicitly pulled in.
    pub fn push_front(&mut self, row: Row) -> Option<Row> {
        if self.capacity == 0 {
            return Some(row);
        }
        let new_head = (self.head + self.capacity - 1) % self.capacity;
        if self.len < self.capacity {
            self.entries[new_head] = Some(row);
            self.head = new_head;
            self.len += 1;
            None
        } else {
            // Full: new_head coincides with the slot of the current newest
            // row. Evict it, write the older row in its place. `head`
            // moves backward so the range still covers `capacity` slots
            // and the newly-inserted row becomes the oldest.
            let evicted = self.entries[new_head].take();
            self.entries[new_head] = Some(row);
            self.head = new_head;
            evicted
        }
    }

    /// Index 0 = oldest, len-1 = newest.
    pub fn get(&self, idx: usize) -> Option<&Row> {
        if idx >= self.len {
            return None;
        }
        let real = (self.head + idx) % self.capacity;
        self.entries[real].as_ref()
    }

    pub fn clear(&mut self) {
        // §B.2 — bump eviction_count by `len` so any extant selection
        // anchor records get invalidated by the same delta-comparison
        // path that handles ordinary scroll-off eviction. Belt+suspenders
        // with `JsTerminal::clear_scrollback`'s explicit `selection.clear()`
        // — covers the case where some other code path triggers a
        // physical clear without going through the JS API.
        self.eviction_count = self.eviction_count.wrapping_add(self.len as u64);
        for slot in &mut self.entries {
            *slot = None;
        }
        self.head = 0;
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_marked(marker: char) -> Row {
        let mut r = Row::new(1);
        r.cells[0].ch = marker;
        r
    }

    #[test]
    fn push_under_capacity() {
        let mut s = Scrollback::new(3);
        assert!(s.push(row_marked('a')).is_none());
        assert!(s.push(row_marked('b')).is_none());
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'a');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'b');
    }

    #[test]
    fn push_evicts_at_capacity() {
        let mut s = Scrollback::new(2);
        assert!(s.push(row_marked('a')).is_none());
        assert!(s.push(row_marked('b')).is_none());
        // Third push evicts 'a'.
        let evicted = s.push(row_marked('c')).unwrap();
        assert_eq!(evicted.cells[0].ch, 'a');
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'b');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'c');
    }

    #[test]
    fn push_front_under_capacity() {
        let mut s = Scrollback::new(3);
        assert!(s.push(row_marked('b')).is_none());
        assert!(s.push(row_marked('c')).is_none());
        // Now order is [b, c]. Prepend 'a' — should land at index 0.
        assert!(s.push_front(row_marked('a')).is_none());
        assert_eq!(s.len(), 3);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'a');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'b');
        assert_eq!(s.get(2).unwrap().cells[0].ch, 'c');
    }

    #[test]
    fn push_front_evicts_newest_at_capacity() {
        let mut s = Scrollback::new(2);
        assert!(s.push(row_marked('b')).is_none());
        assert!(s.push(row_marked('c')).is_none());
        // Full at [b, c]. push_front 'a' evicts the newest ('c').
        let evicted = s.push_front(row_marked('a')).unwrap();
        assert_eq!(evicted.cells[0].ch, 'c');
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'a');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'b');
    }

    #[test]
    fn push_front_into_empty() {
        let mut s = Scrollback::new(3);
        assert!(s.push_front(row_marked('x')).is_none());
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'x');
        // Subsequent push appends at the newest end.
        assert!(s.push(row_marked('y')).is_none());
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'x');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'y');
    }

    #[test]
    fn push_front_multiple_then_push_interleave() {
        let mut s = Scrollback::new(5);
        // Build [c] then prepend b, a → [a, b, c]
        s.push(row_marked('c'));
        s.push_front(row_marked('b'));
        s.push_front(row_marked('a'));
        assert_eq!(s.len(), 3);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'a');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'b');
        assert_eq!(s.get(2).unwrap().cells[0].ch, 'c');
        // Append d, e → [a, b, c, d, e]
        s.push(row_marked('d'));
        s.push(row_marked('e'));
        assert_eq!(s.get(3).unwrap().cells[0].ch, 'd');
        assert_eq!(s.get(4).unwrap().cells[0].ch, 'e');
        // One more push wraps; oldest 'a' is evicted.
        let evicted = s.push(row_marked('f')).unwrap();
        assert_eq!(evicted.cells[0].ch, 'a');
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'b');
        assert_eq!(s.get(4).unwrap().cells[0].ch, 'f');
    }

    #[test]
    fn push_front_capacity_zero_returns_input() {
        let mut s = Scrollback::new(0);
        let returned = s.push_front(row_marked('z')).unwrap();
        assert_eq!(returned.cells[0].ch, 'z');
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn push_capacity_zero_returns_input() {
        // Symmetric to push_front_capacity_zero_returns_input.
        let mut s = Scrollback::new(0);
        let returned = s.push(row_marked('z')).unwrap();
        assert_eq!(returned.cells[0].ch, 'z');
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let mut s = Scrollback::new(3);
        s.push(row_marked('a'));
        s.push(row_marked('b'));
        // len == 2, so idx 0 and 1 are valid; 2, 3, MAX are not.
        assert!(s.get(2).is_none());
        assert!(s.get(3).is_none());
        assert!(s.get(usize::MAX).is_none());
    }

    #[test]
    fn is_empty_tracks_state() {
        let mut s = Scrollback::new(3);
        assert!(s.is_empty());
        s.push(row_marked('a'));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn clear_drops_all_rows() {
        let mut s = Scrollback::new(3);
        s.push(row_marked('a'));
        s.push(row_marked('b'));
        s.push(row_marked('c'));
        assert_eq!(s.len(), 3);
        s.clear();
        assert_eq!(s.len(), 0);
        assert!(s.get(0).is_none());
        // After clear, can still push fresh content.
        s.push(row_marked('x'));
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'x');
    }

    #[test]
    fn eviction_counter_starts_at_zero_and_advances_on_overwrite() {
        // §B.2 — JsTerminal::feed uses this counter to decide whether
        // selection abs-row anchors are still valid.
        let mut s = Scrollback::new(2);
        assert_eq!(s.eviction_count(), 0);
        s.push(row_marked('a'));
        s.push(row_marked('b'));
        // Still under capacity → no eviction yet.
        assert_eq!(s.eviction_count(), 0);
        // Third push overwrites 'a'.
        s.push(row_marked('c'));
        assert_eq!(s.eviction_count(), 1);
        s.push(row_marked('d'));
        assert_eq!(s.eviction_count(), 2);
    }

    #[test]
    fn clear_advances_eviction_counter_by_len() {
        // §B.2 — physical clear is at-least-as-strong an invalidation
        // signal as ordinary eviction. Belt-and-suspenders for
        // selection anchors that didn't go through the JS API path.
        let mut s = Scrollback::new(4);
        s.push(row_marked('a'));
        s.push(row_marked('b'));
        s.push(row_marked('c'));
        let before = s.eviction_count();
        assert_eq!(s.len(), 3);
        s.clear();
        assert_eq!(
            s.eviction_count(),
            before + 3,
            "clear must bump the counter by the number of dropped rows"
        );
    }

    #[test]
    fn push_wraps_head_past_capacity_x_2() {
        // Push 5 rows into capacity 2 — head wraps multiple times.
        // Verifies the modulo head-pointer math doesn't drift.
        let mut s = Scrollback::new(2);
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            s.push(row_marked(ch));
        }
        assert_eq!(s.len(), 2);
        // Last two: 'd', 'e'.
        assert_eq!(s.get(0).unwrap().cells[0].ch, 'd');
        assert_eq!(s.get(1).unwrap().cells[0].ch, 'e');
    }
}
