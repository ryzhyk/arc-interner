![Rust](https://github.com/ryzhyk/arc-interner/workflows/Rust/badge.svg)

# An interner that deallocates unused values.

This crate is a fork of [David Roundy's](https://github.com/droundy/)
[`internment` crate](https://crates.io/crates/internment).
It provides an alternative implementation of the `internment::ArcIntern`
type.  It inherits David's high-level design and API; however it is built
completely on Rust's standard `Arc` and `Mutex` types and does not contain
any unsafe code.

Interning reduces the memory footprint of an application by storing
a unique copy of each distinct value.  It speeds up equality
comparison and hashing operations, as only pointers rather than actual
values need to be compared.  On the flip side, object creation is
slower, as it involves lookup in the interned object pool.

Interning is most commonly applied to strings; however it can also
be useful for other object types.  This library supports interning
of arbitrary objects.

There exist several interning libraries for Rust, each with its own
set of tradeoffs.  This library makes the following design
choices:

- Interned objects are reference counted.  When the last reference to
  an interned object is dropped, the object is deallocated.  This
  prevents unbounded growth of the interned object pool in applications
  where the set of interned values changes dynamically at the cost of
  some CPU and memory overhead (due to storing and maintaining an
  atomic counter).
- Multithreading.  A single pool of interned objects is shared by all
  threads in the program.  This pool is protected by a mutex that is
  acquired every time an object is being interned or a reference to
  an interned object is being dropped.  Althgough Rust mutexes are fairly
  cheap when there is no contention, you may see a significant drop in
  performance under contention.
- Not just strings: this library allows interning any data type that
  satisfies the `Eq + Hash + Send + Sync` trait bound.
- Safe: this library is built on `Arc` and `Mutex` types from the Rust
  standard library and does not contain any unsafe code.

# Example

```rust
use arc_interner::ArcIntern;
let x = ArcIntern::new("hello");
let y = ArcIntern::new("world");
assert_ne!(x, y);
assert_eq!(x, ArcIntern::new("hello"));
assert_eq!(*x, "hello"); // dereference an ArcIntern like a pointer
```
