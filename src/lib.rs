use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use rand::Rng;

/// Implements Google's Jump Consistent Hash algorithm.
/// `num_buckets` must be positive.
fn google_jump_hash(mut key: u64, num_buckets: i32) -> i32 {
  if num_buckets < 1 {
    // This case should ideally be prevented by callers ensuring num_buckets > 0.
    // Or, return an error/option, but for direct port, panic is closer to some C lib behaviors.
    // The original C++ doesn't specify behavior for num_buckets <= 0, implies precondition.
    // The go-jump library panics if num_buckets <= 0.
    panic!("num_buckets must be a positive integer, got {}", num_buckets);
  }

  let mut b: i64 = -1; // bucket number before current jump
  let mut j: i64 = 0; // current bucket number (candidate)

  while j < num_buckets as i64 {
    b = j; // b is the last valid bucket
    key = key.wrapping_mul(2862933555777941757).wrapping_add(1);

    // Calculate the next jump.
    // This matches the C++ reference and common implementations like dgryski/go-jump.
    // The term (double(1LL << 31) / double((key >> 33) + 1))
    // creates a value in [0,1) when ((key >> 33) + 1) is large enough.
    // (1LL << 31) is 2^31.
    let dividend = (1i64 << 31) as f64; // 2147483648.0
                                        // (key >> 33) + 1 can be 0 if key was 0 then became -1 (max u64) after mul/add.
                                        // However, key is u64, so key >> 33 is always non-negative.
                                        // Thus (key >> 33) + 1 is always >= 1.
    let divisor = ((key >> 33) + 1) as f64; // Max val of key >> 33 is 2^31 -1. So divisor is at least 1.0

    // If b+1 is large and (dividend/divisor) is close to 1, j can exceed i32::MAX.
    // Since num_buckets is i32, j should not exceed num_buckets.
    // The intermediate floating point result could be large.
    let jump_ratio = dividend / divisor;
    j = ((b + 1) as f64 * jump_ratio) as i64;

    // Ensure j doesn't become negative due to large float -> i64 conversion if intermediate is huge.
    // This is unlikely given the constraints of jump_ratio being ~[0,1) and b being < num_buckets (i32).
    if j < 0 {
      // This path indicates an extreme edge case or overflow in float to int conversion
      // or if b+1 becomes so large it pushes the product to negative in i64 repr.
      // Given num_buckets is i32, b+1 will be at most i32::MAX + 1.
      // So (b+1) as f64 is fine. Product with ratio near 1 should be okay.
      // For safety, if it became negative and we are still in loop, it implies something went very wrong.
      // Or, if j becomes > num_buckets, the loop terminates.
      // Let's cap j to num_buckets to ensure termination if something is odd with the float math on an arch.
      // The original algo doesn't show this, relies on j eventually exceeding num_buckets.
      j = num_buckets as i64; // Force break if j is problematic
    }
  }
  b as i32
}

/// A multiplier used in the `CompactHolder`'s get method to derive a different
/// hash stream for the same key, reducing correlation with the `LooseHolder`'s hashing.
const COMPACT_HASH_MULTIPLIER: u64 = 0xc6a4a7935bd1e995;

/// Internal structure for the "loose" part of the DoubleJumpHash.
///
/// The loose holder allows for quick additions and removals by potentially
/// leaving empty slots in its arena (`a`). These empty slots can be reused
/// or later removed by `shrink`ing.
#[derive(Debug)]
struct LooseHolder<T: Eq + Hash + Clone> {
  /// The arena storing optional items. `None` represents an empty slot.
  a: Vec<Option<T>>,
  /// Maps an item `T` to its current index in the arena `a`.
  m: HashMap<T, usize>,
  /// A list of indices in `a` that are currently free (contain `None`).
  f: Vec<usize>,
}

impl<T: Eq + Hash + Clone> LooseHolder<T> {
  /// Creates a new, empty `LooseHolder`.
  fn new() -> Self {
    LooseHolder {
      a: Vec::new(),
      m: HashMap::new(),
      f: Vec::new(),
    }
  }

  /// Adds an object to the loose holder.
  ///
  /// If a free slot is available, it's reused. Otherwise, the object
  /// is appended to the arena. If the object already exists,
  /// the operation is a no-op.
  fn add(&mut self, obj: T) {
    if self.m.contains_key(&obj) {
      return;
    }
    if let Some(idx) = self.f.pop() {
      // Try to reuse a free slot
      self.a[idx] = Some(obj.clone());
      self.m.insert(obj, idx);
    } else {
      // Otherwise, append to the end
      self.a.push(Some(obj.clone()));
      self.m.insert(obj, self.a.len() - 1);
    }
  }

  /// Removes an object from the loose holder.
  ///
  /// This marks the object's slot in the arena `a` as `None` and adds
  /// the slot's index to the free list `f`. The object is also removed
  /// from the map `m`.
  fn remove(&mut self, obj: &T) {
    if let Some(idx) = self.m.remove(obj) {
      self.a[idx] = None;
      self.f.push(idx);
    }
  }

  /// Retrieves an object from the loose holder based on a key.
  ///
  /// Uses Google's Jump Consistent Hash to find a slot index.
  /// Returns `Some(T)` if the slot contains an object, `None` if the
  /// slot is empty or the holder itself is empty.
  fn get(&self, key: u64) -> Option<T> {
    if self.a.is_empty() {
      return None;
    }
    let num_buckets = self.a.len() as i32;
    // google_jump_hash panics if num_buckets < 1; this is covered by self.a.is_empty().
    let hash_idx = google_jump_hash(key, num_buckets) as usize;

    // Return a clone of the item if present at the hashed index.
    self.a[hash_idx].as_ref().cloned()
  }

  /// Shrinks the loose holder's arena by removing all empty slots.
  ///
  /// This rebuilds the arena `a` and the map `m` to contain only
  /// active items, and clears the free list `f`.
  /// This operation can be expensive if there are many items.
  fn shrink(&mut self) {
    if self.f.is_empty() {
      // No empty slots to remove
      return;
    }

    let mut new_a: Vec<Option<T>> = Vec::with_capacity(self.m.len());
    let mut new_m: HashMap<T, usize> = HashMap::with_capacity(self.m.len());

    // Iterate through the old arena, keeping only Some(item)
    // This consumes self.a and rebuilds it along with self.m.
    for opt_item_t in std::mem::take(&mut self.a) {
      if let Some(item_t) = opt_item_t {
        let new_idx = new_a.len();
        new_a.push(Some(item_t.clone())); // item_t is cloned into new_a
        new_m.insert(item_t, new_idx); // item_t (original) is moved into new_m
      }
    }

    self.a = new_a;
    self.m = new_m;
    self.f.clear(); // All slots are now compact, so no free slots.
  }
}

/// Internal structure for the "compact" part of the DoubleJumpHash.
///
/// The compact holder always maintains a densely packed list of items.
/// Removals use a swap-remove strategy to keep the list compact.
/// This holder serves as a fallback for lookups if the `LooseHolder`
/// misses due to an empty slot.
#[derive(Debug)]
struct CompactHolder<T: Eq + Hash + Clone> {
  /// The densely packed vector of items.
  a: Vec<T>,
  /// Maps an item `T` to its current index in the vector `a`.
  m: HashMap<T, usize>,
}

impl<T: Eq + Hash + Clone> CompactHolder<T> {
  /// Creates a new, empty `CompactHolder`.
  fn new() -> Self {
    CompactHolder {
      a: Vec::new(),
      m: HashMap::new(),
    }
  }

  /// Adds an object to the compact holder.
  ///
  /// The object is appended to the vector `a`. If the object
  /// already exists, the operation is a no-op.
  fn add(&mut self, obj: T) {
    if self.m.contains_key(&obj) {
      return;
    }
    self.a.push(obj.clone());
    self.m.insert(obj, self.a.len() - 1);
  }

  /// Removes an object from the compact holder.
  ///
  /// Uses a swap-remove strategy on the vector `a` to maintain compactness.
  /// The map `m` is updated accordingly.
  fn remove(&mut self, obj: &T) {
    if let Some(&idx) = self.m.get(obj) {
      // Get current index of obj
      self.m.remove(obj); // Remove obj from map

      // Perform swap_remove on the vector
      let _swapped_out_obj = self.a.swap_remove(idx); // This is the obj that was at `idx`

      // If an element was moved from the end to `idx` (i.e., `idx` was not the last element),
      // update its index in the map.
      if idx < self.a.len() {
        // Check if an element was actually swapped in
        let moved_element = &self.a[idx]; // This is the element that was moved from the end
        self.m.insert(moved_element.clone(), idx);
      }
    }
  }

  /// Retrieves an object from the compact holder based on a key.
  ///
  /// Uses Google's Jump Consistent Hash (with a key multiplier) to find an index.
  /// Returns `Some(T)` if an object is found, `None` if the holder is empty.
  fn get(&self, key: u64) -> Option<T> {
    if self.a.is_empty() {
      return None;
    }
    let num_buckets = self.a.len() as i32;
    // Apply a multiplier to the key to differentiate hashing from LooseHolder
    let h_key = key.wrapping_mul(COMPACT_HASH_MULTIPLIER);
    let hash_idx = google_jump_hash(h_key, num_buckets) as usize;

    self.a.get(hash_idx).cloned()
  }
}

/// A Double Jump consistent hash data structure.
///
/// `DoubleJumpHash` allows for efficient distribution of keys among a
/// dynamically changing set of items `T`. It supports adding and removing
/// items while minimizing key remapping, using an underlying Google's
/// Jump Consistent Hash algorithm.
///
/// Type `T` must implement `Eq`, `Hash`, `Clone`, and `Debug`.
#[derive(Debug)]
pub struct DoubleJumpHash<T: Eq + Hash + Clone> {
  /// The "loose" internal holder, allowing for quick adds/removes with empty slots.
  loose: LooseHolder<T>,
  /// The "compact" internal holder, always densely packed, serving as a fallback.
  compact: CompactHolder<T>,
}

impl<T: Eq + Hash + Clone + Debug> DoubleJumpHash<T> {
  /// Creates a new, empty `DoubleJumpHash`.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  ///
  /// let mut dj_hash: DoubleJumpHash<String> = DoubleJumpHash::new();
  /// ```
  pub fn new() -> Self {
    DoubleJumpHash {
      loose: LooseHolder::new(),
      compact: CompactHolder::new(),
    }
  }

  /// Adds an object to the hash.
  ///
  /// The object is added to both the loose and compact internal holders.
  /// If the object already exists, this operation has no effect.
  ///
  /// # Arguments
  ///
  /// * `obj`: The object of type `T` to add. `T` must be `Clone`.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("server1".to_string());
  /// ```
  pub fn add(&mut self, obj: T) {
    self.loose.add(obj.clone());
    self.compact.add(obj);
  }

  /// Removes an object from the hash.
  ///
  /// The object is removed from both the loose and compact internal holders.
  /// If the object does not exist, this operation has no effect.
  ///
  /// # Arguments
  ///
  /// * `obj`: A reference to the object to remove.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("server1".to_string());
  /// dj_hash.remove(&"server1".to_string());
  /// ```
  pub fn remove(&mut self, obj: &T) {
    self.loose.remove(obj);
    self.compact.remove(obj);
  }

  /// Returns the number of objects currently in the hash.
  ///
  /// This count reflects the number of items in the compact holder.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("nodeA");
  /// dj_hash.add("nodeB");
  /// assert_eq!(dj_hash.len(), 2);
  /// ```
  pub fn len(&self) -> usize {
    self.compact.a.len()
  }

  /// Returns the current capacity of the internal loose object holder's arena.
  ///
  /// This includes active items and any free slots resulting from `remove`
  /// operations that haven't been `shrink`'d. It can be greater than or
  /// equal to `len()`.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("node1");
  /// dj_hash.add("node2");
  /// dj_hash.remove(&"node1");
  /// assert_eq!(dj_hash.len(), 1);
  /// assert_eq!(dj_hash.loose_len(), 2); // "node1" slot is free but part of arena
  /// ```
  pub fn loose_len(&self) -> usize {
    self.loose.a.len()
  }

  /// Shrinks the internal "loose" holder by removing all free slots.
  ///
  /// This operation compacts the loose holder's arena, potentially reclaiming
  /// memory. The compact holder is always compact and is not affected.
  /// This can be useful after many removals to optimize the loose holder,
  /// but may involve reallocations and re-indexing within the loose holder.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("node1");
  /// dj_hash.add("node2");
  /// dj_hash.remove(&"node1");
  /// assert_eq!(dj_hash.loose_len(), 2);
  /// dj_hash.shrink();
  /// assert_eq!(dj_hash.loose_len(), 1);
  /// ```
  pub fn shrink(&mut self) {
    self.loose.shrink();
  }

  /// Retrieves an object from the hash based on a 64-bit key.
  ///
  /// The method first attempts to find a suitable item in the "loose" holder
  /// using Google's Jump Consistent Hash. If the slot found by the hash
  /// function in the loose holder contains an item, that item (cloned) is returned.
  ///
  /// If the loose holder is empty, or if the hashed slot in the loose holder
  /// is empty (e.g., due to a previous removal that has not been `shrink`'d),
  /// the method then queries the "compact" holder using the same hash
  /// algorithm but with a modified key.
  ///
  /// Returns `Some(T)` if an object is found in either holder, or `None` if
  /// the hash is entirely empty or (in rare cases) if both lookups yield empty slots.
  ///
  /// # Arguments
  ///
  /// * `key`: The 64-bit unsigned integer key to look up.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("cache_server_1".to_string());
  /// dj_hash.add("cache_server_2".to_string());
  ///
  /// let user_id: u64 = 12345;
  /// if let Some(server_node) = dj_hash.get(user_id) {
  ///     println!("User {} maps to server: {}", user_id, server_node);
  /// }
  /// ```
  pub fn get(&self, key: u64) -> Option<T> {
    if self.loose.a.is_empty() {
      return self.compact.get(key);
    }

    if let Some(obj) = self.loose.get(key) {
      Some(obj)
    } else {
      self.compact.get(key)
    }
  }

  /// Returns a vector containing clones of all objects currently in the hash.
  ///
  /// The objects are sourced from the "compact" holder. The order of objects
  /// in the returned vector is not guaranteed.
  /// Returns an empty vector if the hash is empty.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("itemA");
  /// dj_hash.add("itemB");
  /// let all_items = dj_hash.all();
  /// assert_eq!(all_items.len(), 2);
  /// ```
  pub fn all(&self) -> Vec<T> {
    self.compact.a.clone()
  }

  /// Returns a randomly selected object from the hash.
  ///
  /// Returns `Some(T)` (a clone of the object) if the hash is not empty,
  /// or `None` otherwise. The selection is made from the "compact" holder.
  ///
  /// # Examples
  ///
  /// ```
  /// use doublejump::DoubleJumpHash; // Corrected crate name
  /// let mut dj_hash = DoubleJumpHash::new();
  /// dj_hash.add("one");
  /// dj_hash.add("two");
  /// dj_hash.add("three");
  /// if let Some(random_item) = dj_hash.random() {
  ///     println!("Random item: {}", random_item);
  /// }
  /// ```
  pub fn random(&self) -> Option<T> {
    if self.compact.a.is_empty() {
      None
    } else {
      let mut rng = rand::rng();
      let idx = rng.random_range(0..self.compact.a.len());
      self.compact.a.get(idx).cloned()
    }
  }
}

/// Implements the `Default` trait for `DoubleJumpHash`.
///
/// Allows creation of an empty `DoubleJumpHash` using `DoubleJumpHash::default()`.
impl<T: Eq + Hash + Clone + Debug> Default for DoubleJumpHash<T> {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Helper for invariant checks (simplified from original Go, can be expanded)
  fn invariant_check<T: Eq + Hash + Clone + Debug>(h: &DoubleJumpHash<T>) {
    assert_eq!(
      h.loose.a.len(),
      h.loose.m.len() + h.loose.f.len(),
      "Loose holder: a.len != m.len + f.len"
    );
    assert_eq!(h.compact.a.len(), h.compact.m.len(), "Compact holder: a.len != m.len");

    for (item, &idx) in &h.loose.m {
      assert!(idx < h.loose.a.len(), "Loose map index out of bounds");
      assert_eq!(h.loose.a[idx].as_ref(), Some(item), "Loose map item mismatch in 'a'");
    }

    let mut free_slots_set = std::collections::HashSet::new();
    for &idx in &h.loose.f {
      assert!(idx < h.loose.a.len(), "Loose free list index out of bounds");
      assert!(h.loose.a[idx].is_none(), "Loose free list slot not None");
      assert!(free_slots_set.insert(idx), "Duplicate index in free list");
    }

    for (item, &idx) in &h.compact.m {
      assert!(idx < h.compact.a.len(), "Compact map index out of bounds");
      assert_eq!(&h.compact.a[idx], item, "Compact map item mismatch in 'a'");
    }
    assert_eq!(h.all().len(), h.len(), "all().len() != len()");
  }

  #[test]
  fn test_hash_new() {
    let h: DoubleJumpHash<i32> = DoubleJumpHash::new();
    assert_eq!(h.len(), 0);
    assert_eq!(h.loose_len(), 0);
    invariant_check(&h);
  }

  #[test]
  fn test_hash_add_basic() {
    let mut h = DoubleJumpHash::new();
    h.add(100);
    assert_eq!(h.len(), 1);
    assert_eq!(h.loose_len(), 1);
    invariant_check(&h);

    h.add(200);
    assert_eq!(h.len(), 2);
    assert_eq!(h.loose_len(), 2);
    invariant_check(&h);

    h.add(100); // Adding same item should not change length
    assert_eq!(h.len(), 2);
    invariant_check(&h);
  }

  #[test]
  fn test_hash_remove_basic() {
    let mut h = DoubleJumpHash::new();
    h.add(100);
    h.add(200);
    h.add(300);
    invariant_check(&h);

    h.remove(&200);
    assert_eq!(h.len(), 2);
    assert_eq!(h.loose_len(), 3);
    invariant_check(&h);
    let all_items = h.all();
    assert!(!all_items.contains(&200));

    h.remove(&100);
    assert_eq!(h.len(), 1);
    assert_eq!(h.loose_len(), 3);
    invariant_check(&h);

    h.remove(&300);
    assert_eq!(h.len(), 0);
    assert_eq!(h.loose_len(), 3);
    invariant_check(&h);
  }

  #[test]
  fn test_hash_get_basic() {
    let mut h = DoubleJumpHash::new();
    assert!(h.get(123).is_none());

    let nodes: Vec<String> = (0..10).map(|i| format!("node{}", i)).collect();
    for node in nodes.iter() {
      h.add(node.clone());
    }
    invariant_check(&h);
    assert_eq!(h.len(), 10);

    for i in 0..30 {
      assert!(h.get(i as u64).is_some(), "Expected Some for key {}", i);
    }

    h.remove(&"node5".to_string());
    h.remove(&"node0".to_string());
    invariant_check(&h);
    assert_eq!(h.len(), 8);

    for i in 0..30 {
      assert!(h.get(i as u64).is_some(), "Expected Some for key {} after removals", i);
    }

    for node in nodes.iter() {
      h.remove(node);
    }
    invariant_check(&h);
    assert_eq!(h.len(), 0);
    assert!(h.get(123).is_none(), "Expected None after all removed");
  }

  #[test]
  fn test_hash_shrink() {
    let mut h = DoubleJumpHash::new();
    for i in 0..10 {
      h.add(i);
    }
    invariant_check(&h);

    for i in 0..5 {
      h.remove(&i);
    }
    assert_eq!(h.len(), 5);
    assert_eq!(h.loose_len(), 10);
    invariant_check(&h);

    h.shrink();
    assert_eq!(h.len(), 5);
    assert_eq!(h.loose_len(), 5);
    assert_eq!(h.loose.f.len(), 0);
    invariant_check(&h);
  }

  #[test]
  fn example_usage() {
    let mut h = DoubleJumpHash::new();
    for i in 0..10 {
      h.add(format!("node{}", i));
    }

    println!("Len: {}", h.len());
    println!("LooseLen: {}", h.loose_len());
    assert_eq!(h.len(), 10);
    assert_eq!(h.loose_len(), 10);

    let g1 = h.get(1000);
    let g2 = h.get(2000);
    let g3 = h.get(3000);
    println!("Get(1000): {:?}", g1); // node9
    println!("Get(2000): {:?}", g2); // node2
    println!("Get(3000): {:?}", g3); // node3

    assert_eq!(g1.unwrap(), "node9".to_string());
    assert_eq!(g2.unwrap(), "node2".to_string());
    assert_eq!(g3.unwrap(), "node3".to_string());

    h.remove(&"node3".to_string());
    println!("Len after remove: {}", h.len());
    println!("LooseLen after remove: {}", h.loose_len());
    assert_eq!(h.len(), 9);
    assert_eq!(h.loose_len(), 10);

    let g4 = h.get(1000);
    let g5 = h.get(2000);
    let g6 = h.get(3000);
    println!("Get(1000) after remove: {:?}", g4); // node9
    println!("Get(2000) after remove: {:?}", g5); // node2
    println!("Get(3000) after remove: {:?}", g6); // node0

    assert_eq!(g4.unwrap(), "node9".to_string());
    assert_eq!(g5.unwrap(), "node2".to_string());
    assert_eq!(g6.unwrap(), "node0".to_string());
  }

  #[test]
  #[should_panic(expected = "num_buckets must be a positive integer, got 0")]
  fn google_jump_hash_zero_buckets() {
    google_jump_hash(123, 0);
  }

  #[test]
  #[should_panic(expected = "num_buckets must be a positive integer, got -5")]
  fn google_jump_hash_negative_buckets() {
    google_jump_hash(123, -5);
  }

  #[test]
  fn google_jump_hash_properties() {
    // Test basic properties of jump hash
    // 1. Output is always in [0, num_buckets)
    let num_buckets = 10;
    for k in 0..1000 {
      let bucket = google_jump_hash(k, num_buckets);
      assert!(bucket >= 0 && bucket < num_buckets);
    }

    // 2. Deterministic
    assert_eq!(google_jump_hash(12345, 50), google_jump_hash(12345, 50));

    // 3. Small number of moves when num_buckets changes (spot check)
    // When num_buckets increases, an object should map to its old bucket or a new, higher one.
    // This is harder to test exhaustively here, but check consistency.
    let key = 789;
    let b10 = google_jump_hash(key, 10);
    let b11 = google_jump_hash(key, 11);
    assert!(b11 == b10 || b11 == 10); // It should map to b10 or the new 11th bucket (index 10)
  }
}
