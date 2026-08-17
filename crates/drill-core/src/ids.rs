//! Stable document identities. Presentation order remains a separate `Vec` index.

use core::num::NonZeroU32;
use serde::{Deserialize, Serialize};

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            pub const fn new(raw: u32) -> Option<Self> {
                match NonZeroU32::new(raw) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                self.get().fmt(formatter)
            }
        }
    };
}

stable_id!(PerformerId);
stable_id!(SetId);
stable_id!(SectionId);
stable_id!(SubsetId);
stable_id!(CameraId);
stable_id!(ProductionMarkerId);
stable_id!(GeneratorId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdAllocator {
    next: u32,
}

const NO_SLOT: u32 = u32::MAX;

#[derive(Clone, Debug, Default)]
pub struct IdIndex {
    base: u32,
    slots: Vec<u32>,
}

impl IdIndex {
    pub fn rebuild<T>(&mut self, values: &[T], id_of: impl Fn(&T) -> u32) {
        let Some(base) = values.iter().map(&id_of).min() else {
            self.base = 0;
            self.slots.clear();
            return;
        };
        let max = values.iter().map(&id_of).max().unwrap_or(base);
        let Some(span) = max.checked_sub(base).and_then(|value| value.checked_add(1)) else {
            self.base = 0;
            self.slots.clear();
            return;
        };
        self.base = base;
        self.slots.clear();
        self.slots.resize(span as usize, NO_SLOT);
        for (index, value) in values.iter().enumerate() {
            if let Ok(index) = u32::try_from(index) {
                self.slots[(id_of(value) - base) as usize] = index;
            }
        }
    }

    pub fn get(&self, raw: u32) -> Option<usize> {
        let offset = raw.checked_sub(self.base)? as usize;
        match self.slots.get(offset).copied() {
            Some(NO_SLOT) | None => None,
            Some(index) => Some(index as usize),
        }
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl IdAllocator {
    pub const fn watermark(self) -> u32 {
        self.next
    }

    pub fn observe(&mut self, raw: u32) {
        if raw >= self.next {
            self.next = raw.saturating_add(1);
        }
    }

    pub fn allocate_performer(&mut self) -> Option<PerformerId> {
        let raw = self.bump()?;
        PerformerId::new(raw)
    }

    pub fn allocate_set(&mut self) -> Option<SetId> {
        let raw = self.bump()?;
        SetId::new(raw)
    }

    pub fn allocate_subset(&mut self) -> Option<SubsetId> {
        let raw = self.bump()?;
        SubsetId::new(raw)
    }

    fn bump(&mut self) -> Option<u32> {
        let raw = self.next;
        self.next = raw.checked_add(1)?;
        Some(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_never_a_valid_identity() {
        assert_eq!(PerformerId::new(0), None);
        assert_eq!(SetId::new(0), None);
    }

    #[test]
    fn allocator_is_monotonic_and_observes_loaded_ids() {
        let mut ids = IdAllocator::default();
        assert_eq!(ids.allocate_performer().unwrap().get(), 1);
        ids.observe(100);
        assert_eq!(ids.allocate_set().unwrap().get(), 101);
    }

    #[test]
    fn index_survives_reordering_and_holes() {
        let ids = [PerformerId::new(9).unwrap(), PerformerId::new(3).unwrap()];
        let mut index = IdIndex::default();
        index.rebuild(&ids, |id| id.get());
        assert_eq!(index.get(3), Some(1));
        assert_eq!(index.get(9), Some(0));
        assert_eq!(index.get(5), None);
    }
}
