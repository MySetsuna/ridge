//! Flyweight interner for `Attrs`.
//!
//! In typical terminal output the vast majority of cells share a small
//! number of distinct attribute combinations (default fg/bg, "red", "bold
//! green", a couple of dim/italic variants for prompts). Interning them
//! collapses 250k cells × 12B = 3 MB of attribute data down to a couple
//! hundred entries plus 250k × 2B `AttrId` indices.

use std::collections::HashMap;

use super::attrs::Attrs;

/// Stable index into `AttrTable`. `0` is reserved for `Attrs::DEFAULT`,
/// which means a freshly-allocated `Cell { attr: AttrId::DEFAULT, .. }`
/// is correct without consulting the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttrId(pub u16);

impl AttrId {
    pub const DEFAULT: AttrId = AttrId(0);
}

impl Default for AttrId {
    fn default() -> Self { AttrId::DEFAULT }
}

pub struct AttrTable {
    /// Index → Attrs. Entry 0 is always `Attrs::DEFAULT`.
    by_id: Vec<Attrs>,
    /// Attrs → Index. HashMap is fine; intern() is hot but bounded by the
    /// number of *distinct* SGR combinations, not by cell count.
    by_attrs: HashMap<Attrs, AttrId>,
}

impl Default for AttrTable {
    fn default() -> Self {
        let mut t = Self {
            by_id: Vec::with_capacity(64),
            by_attrs: HashMap::with_capacity(64),
        };
        // Reserve slot 0 for the default attrs so AttrId::DEFAULT is valid
        // before anyone calls `intern`.
        t.by_id.push(Attrs::DEFAULT);
        t.by_attrs.insert(Attrs::DEFAULT, AttrId::DEFAULT);
        t
    }
}

impl AttrTable {
    /// Insert or look up the index for a given attribute set.
    /// Saturates at u16::MAX — past 65535 distinct combos we recycle the
    /// default. In practice we never come close.
    pub fn intern(&mut self, attrs: Attrs) -> AttrId {
        if let Some(id) = self.by_attrs.get(&attrs) {
            return *id;
        }
        if self.by_id.len() >= u16::MAX as usize {
            return AttrId::DEFAULT;
        }
        let id = AttrId(self.by_id.len() as u16);
        self.by_id.push(attrs);
        self.by_attrs.insert(attrs, id);
        id
    }

    pub fn get(&self, id: AttrId) -> Attrs {
        // Out-of-bounds shouldn't happen if everyone goes through intern,
        // but a corrupt id should not panic on the hot path.
        self.by_id.get(id.0 as usize).copied().unwrap_or(Attrs::DEFAULT)
    }

    pub fn len(&self) -> usize { self.by_id.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::attrs::{Color, Flags};

    #[test]
    fn default_is_id_zero() {
        let mut t = AttrTable::default();
        assert_eq!(t.intern(Attrs::DEFAULT), AttrId::DEFAULT);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn intern_dedupes() {
        let mut t = AttrTable::default();
        let a = Attrs { fg: Color::indexed(1), bg: Color::DEFAULT, flags: Flags::BOLD };
        let id1 = t.intern(a);
        let id2 = t.intern(a);
        assert_eq!(id1, id2);
        assert_eq!(t.len(), 2); // default + a
    }

    #[test]
    fn get_returns_attrs_for_valid_id() {
        let mut t = AttrTable::default();
        let red_bold = Attrs { fg: Color::indexed(1), bg: Color::DEFAULT, flags: Flags::BOLD };
        let id = t.intern(red_bold);
        assert_eq!(t.get(id), red_bold);
        // Default still resolves correctly.
        assert_eq!(t.get(AttrId::DEFAULT), Attrs::DEFAULT);
    }

    #[test]
    fn get_out_of_bounds_returns_default() {
        // Defensive fallback path: if a Cell carries a stale AttrId
        // that points past the table (e.g. after a bug or a
        // sandbox-flush race in prepend_scrollback), get() must NOT
        // panic — it returns Attrs::DEFAULT. Verifies the
        // `unwrap_or(Attrs::DEFAULT)` branch.
        let t = AttrTable::default();
        // Table only has slot 0 (DEFAULT). Asking for slot 100 should
        // fall through to the default.
        assert_eq!(t.get(AttrId(100)), Attrs::DEFAULT);
        assert_eq!(t.get(AttrId(u16::MAX)), Attrs::DEFAULT);
    }

    #[test]
    fn distinct_attrs_produce_distinct_ids() {
        let mut t = AttrTable::default();
        let a = Attrs { fg: Color::indexed(1), bg: Color::DEFAULT, flags: Flags::BOLD };
        let b = Attrs { fg: Color::indexed(2), bg: Color::DEFAULT, flags: Flags::BOLD };
        let c = Attrs { fg: Color::indexed(1), bg: Color::DEFAULT, flags: Flags::ITALIC };
        let id_a = t.intern(a);
        let id_b = t.intern(b);
        let id_c = t.intern(c);
        assert_ne!(id_a, id_b);
        assert_ne!(id_a, id_c);
        assert_ne!(id_b, id_c);
        // None should equal DEFAULT.
        assert_ne!(id_a, AttrId::DEFAULT);
        assert_ne!(id_b, AttrId::DEFAULT);
        assert_ne!(id_c, AttrId::DEFAULT);
    }

    #[test]
    fn multiple_interns_grow_in_insertion_order() {
        let mut t = AttrTable::default();
        let a = Attrs { fg: Color::indexed(1), bg: Color::DEFAULT, flags: Flags::empty() };
        let b = Attrs { fg: Color::indexed(2), bg: Color::DEFAULT, flags: Flags::empty() };
        let c = Attrs { fg: Color::indexed(3), bg: Color::DEFAULT, flags: Flags::empty() };
        let id_a = t.intern(a);
        let id_b = t.intern(b);
        let id_c = t.intern(c);
        assert_eq!(id_a, AttrId(1));
        assert_eq!(id_b, AttrId(2));
        assert_eq!(id_c, AttrId(3));
        assert_eq!(t.len(), 4); // default + 3 interned
    }

    #[test]
    fn rgb_truecolor_attrs_intern_separately() {
        // Each distinct RGB value is its own table entry. With many
        // distinct truecolors (e.g. syntax-highlighted source files)
        // the table grows linearly — bounded only by u16::MAX before
        // the saturation fallback to DEFAULT kicks in.
        let mut t = AttrTable::default();
        let a = Attrs {
            fg: Color::rgb(0x12, 0x34, 0x56),
            bg: Color::DEFAULT,
            flags: Flags::empty(),
        };
        let b = Attrs {
            fg: Color::rgb(0x12, 0x34, 0x57), // differs by one bit
            bg: Color::DEFAULT,
            flags: Flags::empty(),
        };
        let id_a = t.intern(a);
        let id_b = t.intern(b);
        assert_ne!(id_a, id_b);
        // Round-trip both.
        assert_eq!(t.get(id_a), a);
        assert_eq!(t.get(id_b), b);
    }
}
