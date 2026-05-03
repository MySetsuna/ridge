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
}
