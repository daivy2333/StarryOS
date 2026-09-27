//! Bounded workspace-owned CPU mask (Task 4.1 / MS08).
//!
//! The registry `cpumask::CpuMask` guards its indexed `get`/`set` with
//! `debug_assert` only, so release builds silently alias out-of-range indices
//! onto in-range bits (the hart-16 incident). This module contains the crate
//! behind a newtype whose indexed access is capacity-checked in **every**
//! build: membership reads out of range report `false` and mutations return a
//! distinct [`AxCpuMaskError`] without touching the backing store.
//!
//! The registry type never appears in the public API: there is no `Deref`,
//! no `AsRef`, and no raw-inner conversion. `cpumask` remains a private
//! implementation detail of the workspace `axtask` copy.

use core::fmt;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use cpumask::CpuMask;

/// Compile-time capacity of [`AxCpuMask`], taken from the platform
/// configuration. Every indexed access is checked against this bound at
/// runtime in debug and release builds alike.
pub const AX_CPU_MASK_CAPACITY: usize = axconfig::plat::MAX_CPU_NUM;

/// The error returned by checked [`AxCpuMask`] mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxCpuMaskError {
    /// The indexed mutation named a bit at or above the compile-time
    /// capacity. The mask is left completely unchanged.
    IndexOutOfRange {
        /// The rejected index.
        index: usize,
        /// The mask capacity the index was tested against.
        capacity: usize,
    },
}

impl fmt::Display for AxCpuMaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexOutOfRange { index, capacity } => {
                write!(f, "cpu index {index} out of range for mask capacity {capacity}")
            }
        }
    }
}

impl core::error::Error for AxCpuMaskError {}

/// A set of physical CPU ids with unconditional capacity safety.
///
/// This is the workspace replacement for the former public
/// `cpumask::CpuMask` alias. Legal-input behavior (construction, iteration,
/// set algebra, comparisons) matches the registry type; indexed access is
/// fail-closed in every build: reads at or above [`AX_CPU_MASK_CAPACITY`]
/// return `false`, and writes return [`AxCpuMaskError::IndexOutOfRange`]
/// without mutating any legal bit.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct AxCpuMask {
    inner: CpuMask<{ AX_CPU_MASK_CAPACITY }>,
}

impl AxCpuMask {
    /// Construct a mask with every bit set to `false`.
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: CpuMask::new(),
        }
    }

    /// Construct a mask with every bit below the capacity set to `true`.
    #[inline]
    pub fn full() -> Self {
        Self {
            inner: CpuMask::full(),
        }
    }

    /// Construct a mask holding only `index`.
    ///
    /// # Panics
    ///
    /// Panics in every build when `index >= AX_CPU_MASK_CAPACITY`. This
    /// infallible constructor is reserved for call sites that can prove the
    /// index from a source already bounded by the published schedulable set;
    /// dynamic inputs must use [`AxCpuMask::try_one_shot`] and propagate the
    /// error.
    #[inline]
    pub fn one_shot(index: usize) -> Self {
        assert!(
            index < AX_CPU_MASK_CAPACITY,
            "one_shot index {index} out of range for capacity {AX_CPU_MASK_CAPACITY}"
        );
        Self {
            inner: CpuMask::one_shot(index),
        }
    }

    /// Checked variant of [`AxCpuMask::one_shot`]: rejects indices at or
    /// above the capacity with [`AxCpuMaskError::IndexOutOfRange`].
    #[inline]
    pub fn try_one_shot(index: usize) -> Result<Self, AxCpuMaskError> {
        if index >= AX_CPU_MASK_CAPACITY {
            return Err(AxCpuMaskError::IndexOutOfRange {
                index,
                capacity: AX_CPU_MASK_CAPACITY,
            });
        }
        Ok(Self::one_shot(index))
    }

    /// Count the number of set bits.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Test whether the mask contains no set bit.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Test whether every bit below the capacity is set.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    /// Test membership of `index`. Indices at or above the capacity report
    /// `false` in every build instead of reading aliased backing bits.
    #[inline]
    pub fn get(&self, index: usize) -> bool {
        index < AX_CPU_MASK_CAPACITY && self.inner.get(index)
    }

    /// Set the bit `index` to `value`.
    ///
    /// Returns `Ok(previous)` on success. Indices at or above the capacity
    /// return `Err(AxCpuMaskError::IndexOutOfRange)` and leave the complete
    /// mask — every legal bit, the length, iteration order and backing bytes —
    /// unchanged, in debug and release builds alike.
    #[inline]
    pub fn set(&mut self, index: usize, value: bool) -> Result<bool, AxCpuMaskError> {
        if index >= AX_CPU_MASK_CAPACITY {
            return Err(AxCpuMaskError::IndexOutOfRange {
                index,
                capacity: AX_CPU_MASK_CAPACITY,
            });
        }
        Ok(self.inner.set(index, value))
    }

    /// Find the lowest set bit, if any.
    #[inline]
    pub fn first_index(&self) -> Option<usize> {
        self.inner.first_index()
    }

    /// Find the highest set bit, if any.
    #[inline]
    pub fn last_index(&self) -> Option<usize> {
        self.inner.last_index()
    }

    /// Find the lowest set bit strictly above `index`, if any.
    #[inline]
    pub fn next_index(&self, index: usize) -> Option<usize> {
        self.inner.next_index(index)
    }

    /// Get this mask as a slice of its backing bytes (wire representation).
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
}

impl fmt::Debug for AxCpuMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate to the registry formatting to preserve existing log output.
        self.inner.fmt(f)
    }
}

impl PartialOrd for AxCpuMask {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AxCpuMask {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Delegate to the registry's numeric backing-store ordering. The backing
        // bytes are little-endian, so byte-slice lexicographic comparison would
        // order bit 8 below bit 7, opposite to the numeric `u16` store; the
        // registry type orders numerically. Legal masks must order identically.
        self.inner.cmp(&other.inner)
    }
}

impl core::hash::Hash for AxCpuMask {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        // Delegate to the registry's backing-store Hash rather than hashing a
        // native-memory byte slice, so hashing stays source-identical to the
        // replaced `cpumask::CpuMask` at any store width.
        self.inner.hash(state)
    }
}

/// Iterator over the set-bit indices of an [`AxCpuMask`], ascending.
pub struct Iter<'a> {
    mask: &'a AxCpuMask,
    front: Option<usize>,
}

impl Iterator for Iter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let result = match self.front {
            None => self.mask.first_index(),
            Some(index) => {
                if index >= AX_CPU_MASK_CAPACITY {
                    None
                } else {
                    self.mask.next_index(index)
                }
            }
        };
        self.front = match result {
            Some(index) => Some(index),
            None => Some(AX_CPU_MASK_CAPACITY),
        };
        result
    }
}

impl<'a> IntoIterator for &'a AxCpuMask {
    type Item = usize;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            mask: self,
            front: None,
        }
    }
}

macro_rules! delegate_binop {
    ($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident) => {
        impl $trait for AxCpuMask {
            type Output = Self;
            #[inline]
            fn $method(self, rhs: Self) -> Self::Output {
                Self {
                    inner: self.inner.$method(rhs.inner),
                }
            }
        }

        impl $assign_trait for AxCpuMask {
            #[inline]
            fn $assign_method(&mut self, rhs: Self) {
                self.inner.$assign_method(rhs.inner);
            }
        }
    };
}

delegate_binop!(BitAnd, bitand, BitAndAssign, bitand_assign);
delegate_binop!(BitOr, bitor, BitOrAssign, bitor_assign);
delegate_binop!(BitXor, bitxor, BitXorAssign, bitxor_assign);

impl Not for AxCpuMask {
    type Output = Self;
    #[inline]
    fn not(self) -> Self::Output {
        Self {
            inner: self.inner.not(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registry mask built from bit indices, mirroring the type this newtype
    /// replaces. Used as the independent oracle for `Ord`/`Hash` compatibility.
    fn raw_from(indices: &[usize]) -> CpuMask<AX_CPU_MASK_CAPACITY> {
        let mut r = CpuMask::new();
        for &i in indices {
            r.set(i, true);
        }
        r
    }

    fn wrapped_from(indices: &[usize]) -> AxCpuMask {
        let mut m = AxCpuMask::new();
        for &i in indices {
            let _ = m.set(i, true);
        }
        m
    }

    /// The registry type compares its numeric backing store, so bit 7 (`u16`
    /// 0x0080) sorts below bit 8 (`u16` 0x0100). A byte-slice comparison of the
    /// little-endian encoding (`[0x80,0x00]` vs `[0x00,0x01]`) would order them
    /// the opposite way, so this is the cross-byte boundary that must delegate
    /// to the store.
    #[test]
    fn ord_matches_registry_numeric_store_across_byte_boundary() {
        use core::cmp::Ordering;
        // The RED-relevant single pair: numeric store says bit7 < bit8.
        assert_eq!(
            wrapped_from(&[7]).cmp(&wrapped_from(&[8])),
            Ordering::Less,
            "bit 7 must sort below bit 8 (numeric backing store)"
        );

        // Cross-section requirement: every legal mask orders identically to the
        // registry type, including across the single-word byte boundary.
        let cases: [&[usize]; 9] = [&[0], &[1], &[7], &[8], &[15], &[0, 7], &[8, 15], &[7, 8], &[0, 15]];
        for a in &cases {
            for b in &cases {
                assert_eq!(
                    wrapped_from(a).cmp(&wrapped_from(b)),
                    raw_from(a).cmp(&raw_from(b)),
                    "wrapper ordering for {a:?} must equal the registry store ordering"
                );
            }
        }
    }

    /// Hash must delegate to the registry backing-store hash: equal store
    /// values (same bits) hash identically to the replaced type.
    #[test]
    fn hash_delegates_to_registry_backing_store() {
        use core::hash::{Hash, Hasher};

        struct SumHasher(u64);
        impl Hasher for SumHasher {
            fn finish(&self) -> u64 {
                self.0
            }
            fn write(&mut self, bytes: &[u8]) {
                for b in bytes {
                    self.0 = self.0.wrapping_mul(31).wrapping_add(*b as u64);
                }
            }
        }

        fn h<H: Hash>(v: H) -> u64 {
            let mut s = SumHasher(0);
            v.hash(&mut s);
            s.finish()
        }

        let cases: [&[usize]; 7] = [&[0], &[1], &[7], &[8], &[15], &[0, 7], &[8, 15]];
        for c in cases {
            assert_eq!(
                h(&wrapped_from(c)),
                h(&raw_from(c)),
                "wrapper hash for {c:?} must equal the registry backing-store hash"
            );
        }
    }
}
