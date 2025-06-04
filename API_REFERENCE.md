# `doublejump` API Reference

A Rust implementation of the Double Jump consistent hash algorithm, designed to efficiently distribute keys among a changing set of items, allowing for additions and removals.

This data structure uses an underlying implementation of Google's Jump Consistent Hash.

## Table of Contents

1.  [Struct Definition](#struct-definition)
2.  [Creating an Instance](#creating-an-instance)
3.  [Modifying the Hash](#modifying-the-hash)
    *   [`add`](#add)
    *   [`remove`](#remove)
    *   [`shrink`](#shrink)
4.  [Querying the Hash](#querying-the-hash)
    *   [`get`](#get)
    *   [`len`](#len)
    *   [`loose_len`](#loose_len)
    *   [`all`](#all)
    *   [`random`](#random)
5.  [Trait Bounds](#trait-bounds)

---

## Struct Definition

```rust
pub struct DoubleJumpHash<T>
```

Manages a collection of items `T` for consistent hashing.

---

## Creating an Instance

### `new()`

Creates a new, empty `DoubleJumpHash`.

```rust
pub fn new() -> Self
```

**Example:**

```rust
use doublejump::DoubleJumpHash; // Assuming your crate name is doublejump

let mut dj_hash: DoubleJumpHash<String> = DoubleJumpHash::new();
```

The struct also implements `Default`, so `DoubleJumpHash::default()` can be used.

---

## Modifying the Hash

### `add()`

Adds an object to the hash. If the object already exists, this operation has no effect.

```rust
pub fn add(&mut self, obj: T)```

*   `obj`: The object to add. `T` must be `Clone` as it's stored in two internal structures.

**Example:**

```rust
dj_hash.add("server1".to_string());
dj_hash.add("server2".to_string());
```

### `remove()`

Removes an object from the hash. If the object does not exist, this operation has no effect.

```rust
pub fn remove(&mut self, obj: &T)
```

*   `obj`: A reference to the object to remove.

**Example:**

```rust
dj_hash.remove(&"server1".to_string());
```

### `shrink()`

Shrinks the internal "loose" holder by removing free slots created by `remove` operations. This can reclaim memory but may involve reallocations. The "compact" holder is always compact.

```rust
pub fn shrink(&mut self)
```

**Example:**

```rust
dj_hash.shrink();
```

---

## Querying the Hash

### `get()`

Retrieves an object from the hash based on a 64-bit key.

The method first attempts to find a suitable item in the "loose" holder. If the slot found by the hash function in the loose holder is occupied, that item is returned. If the slot is empty (due to a previous removal that has not been `shrink`'d), it then queries the "compact" holder.

Returns `Some(T)` if an object is found, or `None` if the hash is empty or if the key maps to an empty slot in both internal structures (which can happen transiently in the loose holder).

```rust
pub fn get(&self, key: u64) -> Option<T>
```

*   `key`: The 64-bit unsigned integer key to look up.

**Example:**

```rust
let user_id: u64 = 12345;
if let Some(server_node) = dj_hash.get(user_id) {
    println!("User {} maps to server: {}", user_id, server_node);
} else {
    println!("No server available for user {}", user_id); // e.g. if hash is empty
}
```

### `len()`

Returns the number of objects currently in the hash. This reflects the count in the "compact" holder.

```rust
pub fn len(&self) -> usize
```

**Example:**

```rust
let item_count = dj_hash.len();
println!("Number of items: {}", item_count);
```

### `loose_len()`

Returns the current capacity of the internal "loose" object holder's arena. This includes active items and any free slots resulting from `remove` operations that haven't been `shrink`'d.

```rust
pub fn loose_len(&self) -> usize
```

**Example:**

```rust
let loose_capacity = dj_hash.loose_len();
println!("Loose holder capacity: {}", loose_capacity);
```

### `all()`

Returns a `Vec<T>` containing all objects currently in the hash. The objects are cloned from the "compact" holder. The order of objects in the returned vector is not guaranteed.

Returns an empty vector if the hash is empty.

```rust
pub fn all(&self) -> Vec<T>
```

**Example:**

```rust
let all_nodes = dj_hash.all();
for node in all_nodes {
    println!("Node: {}", node);
}
```

### `random()`

Returns a random object from the hash.
Returns `Some(T)` if the hash is not empty, or `None` otherwise. The object is cloned.

```rust
pub fn random(&self) -> Option<T>
```

**Example:**

```rust
if let Some(random_node) = dj_hash.random() {
    println!("Randomly selected node: {}", random_node);
}
```

---

## Trait Bounds

The generic type `T` used in `DoubleJumpHash<T>` must satisfy the following trait bounds:

*   `Eq`: For equality comparisons (used extensively in `HashMap` lookups and internal logic).
*   `Hash`: To be usable as a key in `HashMap`.
*   `Clone`: As items are stored in multiple places and returned by value from some methods (e.g., `get`, `all`, `random`).
*   `Debug` (optional, but required for the default `DoubleJumpHash` tests and if you want to print the hash structure itself using `{:?}`). The public API provided in the crate requires `Debug` for `DoubleJumpHash<T>`.

```rust
pub struct DoubleJumpHash<T: Eq + Hash + Clone + Debug> { /* ... */ }
```