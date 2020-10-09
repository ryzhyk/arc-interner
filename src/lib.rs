//! An interner that deallocates unused values.
//!
//! This crate is a fork of [David Roundy's](https://github.com/droundy/)
//! [`internment` crate](https://crates.io/crates/internment).
//! It provides an alternative implementation of the `internment::ArcIntern`
//! type.  It inherits David's high-level design and API; however it is built
//! completely on Rust's standard `Arc` and the
//! [`dashmap` crate](https://crates.io/crates/dashmap) and does not contain
//! any unsafe code.
//!
//! Interning reduces the memory footprint of an application by storing
//! a unique copy of each distinct value.  It speeds up equality
//! comparison and hashing operations, as only pointers rather than actual
//! values need to be compared.  On the flip side, object creation is
//! slower, as it involves lookup in the interned object pool.
//!
//! Interning is most commonly applied to strings; however it can also
//! be useful for other object types.  This library supports interning
//! of arbitrary objects.
//!
//! There exist several interning libraries for Rust, each with its own
//! set of tradeoffs.  This library makes the following design
//! choices:
//!
//! - Interned objects are reference counted.  When the last reference to
//!   an interned object is dropped, the object is deallocated.  This
//!   prevents unbounded growth of the interned object pool in applications
//!   where the set of interned values changes dynamically at the cost of
//!   some CPU and memory overhead (due to storing and maintaining an
//!   atomic counter).
//! - Multithreading.  A single pool of interned objects is shared by all
//!   threads in the program.  Inside `DashMap` this pool is protected by
//!   mutexes that are acquired every time an object is being interned or a
//!   reference to an interned object is being dropped.  Although Rust mutexes
//!   are fairly cheap when there is no contention, you may see a significant
//!   drop in performance under contention.
//! - Not just strings: this library allows interning any data type that
//!   satisfies the `Eq + Hash + Send + Sync` trait bound.
//! - Safe: this library is built on `Arc` type from the Rust
//!   standard library and the [`dashmap` crate](https://crates.io/crates/dashmap)
//!   and does not contain any unsafe code (although std and dashmap do of course)
//!
//! # Example
//! ```rust
//! use arc_interner::ArcIntern;
//! let x = ArcIntern::new("hello");
//! let y = ArcIntern::new("world");
//! assert_ne!(x, y);
//! assert_eq!(x, ArcIntern::new("hello"));
//! assert_eq!(*x, "hello"); // dereference an ArcIntern like a pointer
//! ```

use dashmap::DashMap;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::{Any, TypeId};
use std::borrow::Borrow;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

/// A pointer to a reference-counted interned object.
///
/// The interned object will be held in memory only until its
/// reference count reaches zero.
///
/// # Example
/// ```rust
/// use arc_interner::ArcIntern;
///
/// let x = ArcIntern::new("hello");
/// let y = ArcIntern::new("world");
/// assert_ne!(x, y);
/// assert_eq!(x, ArcIntern::new("hello"));
/// assert_eq!(*x, "hello"); // dereference an ArcIntern like a pointer
/// ```
#[derive(Debug)]
pub struct ArcIntern<T: Eq + Hash + Send + Sync + 'static> {
    arc: Arc<T>,
}

type Container<T> = DashMap<Arc<T>, ()>;

static CONTAINER: OnceCell<DashMap<TypeId, Box<dyn Any + Send + Sync>>> = OnceCell::new();

impl<T: Eq + Hash + Send + Sync + 'static> ArcIntern<T> {
    /// Intern a value.  If this value has not previously been
    /// interned, then `new` will allocate a spot for the value on the
    /// heap.  Otherwise, it will return a pointer to the object
    /// previously allocated.
    ///
    /// Note that `ArcIntern::new` is a bit slow, since it needs to check
    /// a `DashMap` which contains its own mutexes.
    pub fn new(val: T) -> ArcIntern<T> {
        let type_map = CONTAINER.get_or_init(|| DashMap::new());

        // Prefer taking the read lock to reduce contention, only use entry api if necessary.
        let boxed = if let Some(boxed) = type_map.get(&TypeId::of::<T>()) {
            boxed
        } else {
            type_map
                .entry(TypeId::of::<T>())
                .or_insert_with(|| Box::new(Container::<T>::new()))
                .downgrade()
        };

        let m: &Container<T> = boxed.value().downcast_ref::<Container<T>>().unwrap();
        let b = m.entry(Arc::new(val)).or_insert(());
        return ArcIntern {
            arc: b.key().clone(),
        };
    }
    /// See how many objects have been interned.  This may be helpful
    /// in analyzing memory use.
    pub fn num_objects_interned() -> usize {
        if let Some(m) = CONTAINER
            .get()
            .and_then(|type_map| type_map.get(&TypeId::of::<T>()))
        {
            return m.downcast_ref::<Container<T>>().unwrap().len();
        }
        0
    }
    /// Return the number of references for this value.
    pub fn refcount(&self) -> usize {
        // One reference is held by the hashset; we return the number of
        // references held by actual clients.
        Arc::strong_count(&self.arc) - 1
    }
}

impl<T: Eq + Hash + Send + Sync + 'static> Clone for ArcIntern<T> {
    fn clone(&self) -> Self {
        ArcIntern {
            arc: self.arc.clone(),
        }
    }
}

impl<T: Eq + Hash + Send + Sync> Drop for ArcIntern<T> {
    fn drop(&mut self) {
        if let Some(m) = CONTAINER
            .get()
            .and_then(|type_map| type_map.get(&TypeId::of::<T>()))
        {
            let m: &Container<T> = m.downcast_ref::<Container<T>>().unwrap();
            m.remove_if(&self.arc, |k, _v| {
                // If the reference count is 2, then the only two remaining references
                // to this value are held by `self` and the hashmap and we can safely
                // deallocate the value.
                Arc::strong_count(&k) == 2
            });
        }
    }
}

impl<T: Send + Sync + Hash + Eq> AsRef<T> for ArcIntern<T> {
    fn as_ref(&self) -> &T {
        self.arc.as_ref()
    }
}
impl<T: Eq + Hash + Send + Sync> Borrow<T> for ArcIntern<T> {
    fn borrow(&self) -> &T {
        self.as_ref()
    }
}
impl<T: Eq + Hash + Send + Sync> Deref for ArcIntern<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.as_ref()
    }
}

impl<T: Eq + Hash + Send + Sync + Display> Display for ArcIntern<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        self.deref().fmt(f)
    }
}

impl<T: Eq + Hash + Send + Sync + 'static> From<T> for ArcIntern<T> {
    fn from(t: T) -> Self {
        ArcIntern::new(t)
    }
}
impl<T: Eq + Hash + Send + Sync + Default + 'static> Default for ArcIntern<T> {
    fn default() -> ArcIntern<T> {
        ArcIntern::new(Default::default())
    }
}

/// The hash implementation returns the hash of the pointer
/// value, not the hash of the value pointed to.  This should
/// be irrelevant, since there is a unique pointer for every
/// value, but it *is* observable, since you could compare the
/// hash of the pointer with hash of the data itself.
impl<T: Eq + Hash + Send + Sync> Hash for ArcIntern<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let inner: &T = self.arc.deref();
        inner.hash(state)
    }
}

/// Efficiently compares two interned values by comparing their pointers.
impl<T: Eq + Hash + Send + Sync> PartialEq for ArcIntern<T> {
    fn eq(&self, other: &ArcIntern<T>) -> bool {
        Arc::ptr_eq(&self.arc, &other.arc)
    }
}
impl<T: Eq + Hash + Send + Sync> Eq for ArcIntern<T> {}

impl<T: Eq + Hash + Send + Sync + PartialOrd> PartialOrd for ArcIntern<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_ref().partial_cmp(other)
    }
    fn lt(&self, other: &Self) -> bool {
        self.as_ref().lt(other)
    }
    fn le(&self, other: &Self) -> bool {
        self.as_ref().le(other)
    }
    fn gt(&self, other: &Self) -> bool {
        self.as_ref().gt(other)
    }
    fn ge(&self, other: &Self) -> bool {
        self.as_ref().ge(other)
    }
}

impl<T: Eq + Hash + Send + Sync + Ord> Ord for ArcIntern<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_ref().cmp(other)
    }
}

impl<T: Eq + Hash + Send + Sync + Serialize> Serialize for ArcIntern<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<'de, T: Eq + Hash + Send + Sync + 'static + Deserialize<'de>> Deserialize<'de>
    for ArcIntern<T>
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(|x: T| Self::new(x))
    }
}

#[cfg(test)]
mod tests {
    use crate::ArcIntern;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread;

    // Test basic functionality.
    #[test]
    fn basic() {
        assert_eq!(ArcIntern::new("foo"), ArcIntern::new("foo"));
        assert_ne!(ArcIntern::new("foo"), ArcIntern::new("bar"));
        // The above refs should be deallocate by now.
        assert_eq!(ArcIntern::<&str>::num_objects_interned(), 0);

        let _interned1 = ArcIntern::new("foo".to_string());
        {
            let interned2 = ArcIntern::new("foo".to_string());
            let interned3 = ArcIntern::new("bar".to_string());

            assert_eq!(interned2.refcount(), 2);
            assert_eq!(interned3.refcount(), 1);
            // We now have two unique interned strings: "foo" and "bar".
            assert_eq!(ArcIntern::<String>::num_objects_interned(), 2);
        }

        // "bar" is now gone.
        assert_eq!(ArcIntern::<String>::num_objects_interned(), 1);
    }

    // Ordering should be based on values, not pointers.
    // Also tests `Display` implementation.
    #[test]
    fn sorting() {
        let mut interned_vals = vec![
            ArcIntern::new(4),
            ArcIntern::new(2),
            ArcIntern::new(5),
            ArcIntern::new(0),
            ArcIntern::new(1),
            ArcIntern::new(3),
        ];
        interned_vals.sort();
        let sorted: Vec<String> = interned_vals.iter().map(|v| format!("{}", v)).collect();
        assert_eq!(&sorted.join(","), "0,1,2,3,4,5");
    }

    #[derive(Eq, PartialEq, Hash)]
    pub struct TestStruct2(String, u64);

    #[test]
    fn sequential() {
        for _i in 0..10_000 {
            let mut interned = Vec::with_capacity(100);
            for j in 0..100 {
                interned.push(ArcIntern::new(TestStruct2("foo".to_string(), j)));
            }
        }

        assert_eq!(ArcIntern::<TestStruct2>::num_objects_interned(), 0);
    }

    #[derive(Eq, PartialEq, Hash)]
    pub struct TestStruct(String, u64, Arc<bool>);

    // Quickly create and destroy a small number of interned objects from
    // multiple threads.
    #[test]
    fn multithreading1() {
        let mut thandles = vec![];
        let drop_check = Arc::new(true);
        for _i in 0..10 {
            let t = thread::spawn({
                let drop_check = drop_check.clone();
                move || {
                    for _i in 0..100_000 {
                        let interned1 =
                            ArcIntern::new(TestStruct("foo".to_string(), 5, drop_check.clone()));
                        let _interned2 =
                            ArcIntern::new(TestStruct("bar".to_string(), 10, drop_check.clone()));
                        let mut m = HashMap::new();
                        // force some hashing
                        m.insert(interned1, ());
                    }
                }
            });
            thandles.push(t);
        }
        for h in thandles.into_iter() {
            h.join().unwrap()
        }
        assert_eq!(Arc::strong_count(&drop_check), 1);
        assert_eq!(ArcIntern::<TestStruct>::num_objects_interned(), 0);
    }
}
