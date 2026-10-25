//! Generational slot storage. Handles stay small and copyable, and a
//! handle to a removed value is detected instead of silently aliasing
//! whatever reused its slot.
//!
//!   insert A → {index 0, gen 0}
//!   remove A → slot 0 freed, gen bumped to 1
//!   insert B → {index 0, gen 1}   old handle {0, 0} now misses

/// Handle into [`Slots`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId {
    index: u32,
    generation: u32,
}

impl SlotId {
    /// The handle packed into a u64, e.g. to pass through a script.
    /// Opaque: only [`SlotId::from_bits`] should read it.
    pub fn to_bits(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }

    /// Unpacks [`SlotId::to_bits`]. Garbage gives a handle that misses.
    pub fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

struct Entry<T> {
    generation: u32,
    value: Option<T>,
}

/// Vec-backed storage addressed by [`SlotId`]. Freed slots are reused.
pub struct Slots<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Slots<T> {
    /// Stores `value`, reusing a freed slot when one exists.
    pub fn insert(&mut self, value: T) -> SlotId {
        if let Some(index) = self.free.pop() {
            let entry = &mut self.entries[index as usize];
            entry.value = Some(value);

            return SlotId {
                index,
                generation: entry.generation,
            };
        }

        let index = self.entries.len() as u32;
        self.entries.push(Entry {
            generation: 0,
            value: Some(value),
        });

        SlotId {
            index,
            generation: 0,
        }
    }

    /// The value for `id`, or None if it was removed.
    pub fn get(&self, id: SlotId) -> Option<&T> {
        let entry = self.entries.get(id.index as usize)?;
        if entry.generation != id.generation {
            return None;
        }

        entry.value.as_ref()
    }

    /// Mutable access, e.g. to replace a value in place and keep its handle.
    pub fn get_mut(&mut self, id: SlotId) -> Option<&mut T> {
        let entry = self.entries.get_mut(id.index as usize)?;
        if entry.generation != id.generation {
            return None;
        }

        entry.value.as_mut()
    }

    /// Removes and returns the value. Stale or repeated removes return None.
    pub fn remove(&mut self, id: SlotId) -> Option<T> {
        let entry = self.entries.get_mut(id.index as usize)?;
        if entry.generation != id.generation {
            return None;
        }

        let value = entry.value.take()?;
        entry.generation = entry.generation.wrapping_add(1);
        self.free.push(id.index);

        Some(value)
    }

    /// Number of live values.
    pub fn len(&self) -> usize {
        self.entries.len() - self.free.len()
    }

    /// True when no values are stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Live values with their handles, in slot order (stable between
    /// calls while nothing is inserted or removed).
    pub fn iter(&self) -> impl Iterator<Item = (SlotId, &T)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let id = SlotId {
                    index: index as u32,
                    generation: entry.generation,
                };
                entry.value.as_ref().map(|value| (id, value))
            })
    }

    /// As `iter`, with mutable values.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (SlotId, &mut T)> {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(index, entry)| {
                let id = SlotId {
                    index: index as u32,
                    generation: entry.generation,
                };
                entry.value.as_mut().map(|value| (id, value))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_round_trip_through_bits() {
        let mut slots = Slots::default();
        slots.insert("a");
        let b = slots.insert("b");
        slots.remove(b);
        let c = slots.insert("c");

        let back = SlotId::from_bits(c.to_bits());
        assert_eq!(back, c);
        assert_eq!(slots.get(back), Some(&"c"));
    }

    #[test]
    fn iter_skips_removed_slots_and_yields_live_handles() {
        let mut slots = Slots::default();
        let a = slots.insert("a");
        let b = slots.insert("b");
        let c = slots.insert("c");
        slots.remove(b);

        let live: Vec<_> = slots.iter().collect();
        assert_eq!(live, [(a, &"a"), (c, &"c")]);

        for (_, value) in slots.iter_mut() {
            *value = "z";
        }
        assert_eq!(slots.get(c), Some(&"z"));
    }

    #[test]
    fn insert_then_get() {
        let mut slots = Slots::default();
        let a = slots.insert("a");
        let b = slots.insert("b");

        assert_eq!(slots.get(a), Some(&"a"));
        assert_eq!(slots.get(b), Some(&"b"));
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn stale_handle_misses_after_reuse() {
        let mut slots = Slots::default();
        let a = slots.insert("a");

        assert_eq!(slots.remove(a), Some("a"));
        let b = slots.insert("b");

        assert_eq!(slots.get(a), None);
        assert_eq!(slots.get(b), Some(&"b"));
        assert_eq!(slots.len(), 1);
    }

    #[test]
    fn get_mut_replaces_in_place_and_respects_generation() {
        let mut slots = Slots::default();
        let a = slots.insert("a");

        *slots.get_mut(a).unwrap() = "b";
        assert_eq!(slots.get(a), Some(&"b"));

        slots.remove(a);
        assert!(slots.get_mut(a).is_none());
    }

    #[test]
    fn double_remove_is_harmless() {
        let mut slots = Slots::default();
        let a = slots.insert(1);

        assert_eq!(slots.remove(a), Some(1));
        assert_eq!(slots.remove(a), None);
        assert!(slots.is_empty());
    }
}
