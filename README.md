# DoubleJump

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/doublejump.svg)](https://crates.io/crates/doublejump)
[![Docs.rs](https://docs.rs/doublejump/badge.svg)](https://docs.rs/doublejump)

`doublejump` is a Rust implementation of the "Double Jump" consistent hashing algorithm. This algorithm is designed to efficiently distribute keys among a dynamically changing set of target items (often servers or nodes), particularly excelling in scenarios where items can be added and removed.

This crate provides a data structure, `DoubleJumpHash<T>`, that implements this algorithm. It is inspired by and aims to be a faithful port of the Go [doublejump package](https://pkg.go.dev/github.com/rekby/doublejump), which itself is a revamped version of Google's Jump Consistent Hash that elegantly handles node removals.

## Features

*   **Consistent Hashing**: Minimizes key remapping when the set of target items changes.
*   **Efficient Node Removal**: Supports adding and removing items without significant performance degradation or remapping storms.
*   **Two-Holder Strategy**: Utilizes a "loose" holder for quick additions/removals and a "compact" holder for optimal lookup performance, balancing trade-offs.
*   **Internal Google Jump Hash**: Uses an embedded implementation of Google's Jump Consistent Hash, requiring no external C dependencies or complex cryptographic hash functions.
*   **Generic**: Works with any item type `T` that implements `Eq + Hash + Clone + Debug`.
*   **Simple API**: Easy to integrate and use.

## Why use DoubleJump?

Traditional consistent hashing algorithms can be complex to implement or may have limitations with frequent node additions and removals. Google's Jump Consistent Hash is very fast and simple but inherently doesn't support node removal without remapping many keys.

`doublejump` provides a practical solution by layering a clever two-structure approach on top of the Jump Consistent Hash logic, allowing for efficient removals while retaining good distribution properties.

## Installation

Add `doublejump` to your `Cargo.toml`:

```toml
[dependencies]
doublejump = "0.1.0" # Replace with the latest version
```
*(Please replace `"0.1.0"` with the actual version from Crates.io if you publish it, and ensure the crate name `doublejump` matches your published name.)*

## Usage

Here's a basic example:

```rust
use doublejump::DoubleJumpHash; // Adjust if your crate name is different in Cargo.toml

fn main() {
    // Create a new hash ring for String items
    let mut dj_hash: DoubleJumpHash<String> = DoubleJumpHash::new();

    // Add some nodes
    dj_hash.add("server-alpha".to_string());
    dj_hash.add("server-beta".to_string());
    dj_hash.add("server-gamma".to_string());

    println!("Number of nodes: {}", dj_hash.len()); // Output: 3

    // Get a node for a given key
    let key1: u64 = 12345;
    if let Some(node) = dj_hash.get(key1) {
        println!("Key {} is mapped to: {}", key1, node);
    }

    // Remove a node
    dj_hash.remove(&"server-beta".to_string());
    println!("Number of nodes after removal: {}", dj_hash.len()); // Output: 2

    // Key mapping might change or stay the same depending on which node was hit
    if let Some(node_after_removal) = dj_hash.get(key1) {
        println!("Key {} is now mapped to: {}", key1, node_after_removal);
    }

    // Add a new node
    dj_hash.add("server-delta".to_string());
    println!("Number of nodes after adding delta: {}", dj_hash.len()); // Output: 3

    // Optionally, shrink to reclaim space from removed nodes in the loose holder
    dj_hash.shrink();
}
```

## API Highlights

The `DoubleJumpHash<T>` struct provides the following key methods:

*   `new() -> Self`: Creates a new instance.
*   `add(&mut self, obj: T)`: Adds an item.
*   `remove(&mut self, obj: &T)`: Removes an item.
*   `get(&self, key: u64) -> Option<T>`: Gets the item for a key.
*   `len(&self) -> usize`: Returns the number of items.
*   `loose_len(&self) -> usize`: Returns the capacity of the internal "loose" arena.
*   `shrink(&mut self)`: Compacts the "loose" holder.
*   `all(&self) -> Vec<T>`: Returns all items.
*   `random(&self) -> Option<T>`: Returns a random item.

For detailed API documentation, please refer to [docs.rs/doublejump](https://docs.rs/doublejump) (link will be active once published).

## Algorithm Background

The core idea of this "Double Jump" approach is to maintain two internal structures:

1.  **Loose Holder**: An array-like structure that can contain empty slots. Additions fill empty slots or append. Removals mark slots as empty. This makes additions and removals very fast. Lookups here use Google's Jump Hash. If a lookup hits an empty slot, it's a miss.
2.  **Compact Holder**: A tightly packed array of items, also using Google's Jump Hash (with a different seed/key multiplier) for lookups. This is the fallback if the loose holder misses.

This combination provides the benefits of quick updates (loose holder) and guaranteed finds if the item exists (compact holder), while ensuring the properties of consistent hashing.

## Contributing

Contributions are welcome! Please feel free to submit issues, fork the repository, and create pull requests.

When contributing, please ensure that your code adheres to the existing style and that all tests pass.

## License

This project is licensed under the **Mozilla Public License Version 2.0 (MPL 2.0)**.

See the [LICENSE](LICENSE) file for details.