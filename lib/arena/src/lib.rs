//! Yet another index-based arena.

#![warn(missing_docs)]

use std::{
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    marker::PhantomData,
    ops::{Index, IndexMut},
};

mod map;
pub use map::ArenaMap;

/// The raw index of a value in an arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawIdx(u32);

impl From<RawIdx> for u32 {
    fn from(raw: RawIdx) -> u32 {
        raw.0
    }
}

impl From<u32> for RawIdx {
    fn from(idx: u32) -> RawIdx {
        RawIdx(idx)
    }
}

impl fmt::Debug for RawIdx {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for RawIdx {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The index of a value allocated in an arena that holds `T`s.
pub struct Idx<T> {
    raw: RawIdx,
    _ty: PhantomData<fn() -> T>,
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Idx<T> {}

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Idx<T>) -> bool {
        self.raw == other.raw
    }
}
impl<T> Eq for Idx<T> {}

impl<T> Hash for Idx<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut type_name = std::any::type_name::<T>();
        if let Some(idx) = type_name.rfind(':') {
            type_name = &type_name[idx + 1..];
        }
        write!(f, "Idx::<{}>({})", type_name, self.raw)
    }
}

impl<T> Idx<T> {
    /// Creates a new index from a [`RawIdx`].
    pub fn from_raw(raw: RawIdx) -> Self {
        Idx { raw, _ty: PhantomData }
    }

    /// Converts this index into the underlying [`RawIdx`].
    pub fn into_raw(self) -> RawIdx {
        self.raw
    }
}

/// Yet another index-based arena.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Arena<T> {
    data: Vec<T>,
}

impl<T: fmt::Debug> fmt::Debug for Arena<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Arena").field("len", &self.len()).field("data", &self.data).finish()
    }
}

impl<T> Arena<T> {
    /// Creates a new empty arena.
    ///
    /// ```
    /// let arena: la_arena::Arena<i32> = la_arena::Arena::new();
    /// assert!(arena.is_empty());
    /// ```
    pub const fn new() -> Arena<T> {
        Arena { data: Vec::new() }
    }

    /// Empties the arena, removing all contained values.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    ///
    /// arena.alloc(1);
    /// arena.alloc(2);
    /// arena.alloc(3);
    /// assert_eq!(arena.len(), 3);
    ///
    /// arena.clear();
    /// assert!(arena.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Returns the length of the arena.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    /// assert_eq!(arena.len(), 0);
    ///
    /// arena.alloc("foo");
    /// assert_eq!(arena.len(), 1);
    ///
    /// arena.alloc("bar");
    /// assert_eq!(arena.len(), 2);
    ///
    /// arena.alloc("baz");
    /// assert_eq!(arena.len(), 3);
    /// ```
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns whether the arena contains no elements.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    /// assert!(arena.is_empty());
    ///
    /// arena.alloc(0.5);
    /// assert!(!arena.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Allocates a new value on the arena, returning the value’s index.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    /// let idx = arena.alloc(50);
    ///
    /// assert_eq!(arena[idx], 50);
    /// ```
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        let idx = RawIdx(self.data.len() as u32);
        self.data.push(value);
        Idx::from_raw(idx)
    }

    /// Returns an iterator over the arena’s elements.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    /// let idx1 = arena.alloc(20);
    /// let idx2 = arena.alloc(40);
    /// let idx3 = arena.alloc(60);
    ///
    /// let mut iterator = arena.iter();
    /// assert_eq!(iterator.next(), Some((idx1, &20)));
    /// assert_eq!(iterator.next(), Some((idx2, &40)));
    /// assert_eq!(iterator.next(), Some((idx3, &60)));
    /// ```
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (Idx<T>, &T)> + ExactSizeIterator + DoubleEndedIterator {
        self.data.iter().enumerate().map(|(idx, value)| (Idx::from_raw(RawIdx(idx as u32)), value))
    }

    /// Returns an iterator over the arena’s mutable elements.
    ///
    /// ```
    /// let mut arena = la_arena::Arena::new();
    /// let idx1 = arena.alloc(20);
    ///
    /// assert_eq!(arena[idx1], 20);
    ///
    /// let mut iterator = arena.iter_mut();
    /// *iterator.next().unwrap().1 = 10;
    /// drop(iterator);
    ///
    /// assert_eq!(arena[idx1], 10);
    /// ```
    pub fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (Idx<T>, &mut T)> + ExactSizeIterator + DoubleEndedIterator {
        self.data
            .iter_mut()
            .enumerate()
            .map(|(idx, value)| (Idx::from_raw(RawIdx(idx as u32)), value))
    }

    /// Reallocates the arena to make it take up as little space as possible.
    pub fn shrink_to_fit(&mut self) {
        self.data.shrink_to_fit();
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Arena<T> {
        Arena { data: Vec::new() }
    }
}

impl<T> Index<Idx<T>> for Arena<T> {
    type Output = T;
    fn index(&self, idx: Idx<T>) -> &T {
        let idx = idx.into_raw().0 as usize;
        &self.data[idx]
    }
}

impl<T> IndexMut<Idx<T>> for Arena<T> {
    fn index_mut(&mut self, idx: Idx<T>) -> &mut T {
        let idx = idx.into_raw().0 as usize;
        &mut self.data[idx]
    }
}

impl<T> FromIterator<T> for Arena<T> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        Arena { data: Vec::from_iter(iter) }
    }
}
