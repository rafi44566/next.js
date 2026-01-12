//! Task Storage Schema Definition
//!
//! This module defines the complete schema for task storage using the TaskStorage derive macro.
//! The schema covers all 37 CachedDataItem variants with appropriate storage types and categories.
//!
//! # Storage Types (`storage = "..."`)
//!
//! - `direct` - For single optional values (e.g., Output, Dirty, AggregationNumber)
//! - `auto_set` - For sets of keys with unit values (e.g., Child, OutputDependency)
//! - `counter_map` - For maps with counted references (e.g., Upper, Follower, Collectible)
//! - `auto_map` - For maps with non-counter values (e.g., CellData)
//! - `auto_multimap` - For maps with set values (e.g., CellDependents)
//! - `flag` - For boolean flags stored in TaskFlags bitfield
//!
//! # Categories (`category = "..."`)
//!
//! - `data` - Frequently changed bulk data (dependencies, cell data)
//! - `meta` - Rarely changed metadata (output, aggregation, flags)
//! - `transient` - Not serialized, only exists in memory

use rustc_hash::FxHashSet;
use turbo_tasks::{
    CellId, SharedReference, TaskId, TraitTypeId, TypedSharedReference, ValueTypeId,
};
use turbo_tasks_macros::TaskStorage;

use crate::data::{
    ActivenessState, AggregationNumber, CellRef, CollectibleRef, CollectiblesRef, Dirtyness,
    InProgressCellState, InProgressState, OutputValue,
};

/// Auto-set storage for small sets of keys with unit values.
/// Optimized for small collections (< 8 items use SmallVec inline).
pub type AutoSet<K> = FxHashSet<K>;

/// Counter map storage for reference-counted associations.
/// Automatically removes entries when count reaches 0.
pub type CounterMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// Auto-map storage for key-value pairs.
pub type AutoMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// Auto-multimap storage for key -> set-of-values pairs.
/// A map where each key maps to a set of values (one-to-many relationship).
pub type AutoMultimap<K, V> = rustc_hash::FxHashMap<K, rustc_hash::FxHashSet<V>>;

/// The complete task storage schema.
///
/// This struct defines all storage fields for a task. The TaskStorage macro
/// generates the TaskStorage struct, LazyField enum, and all accessor methods.
///
/// Fields are stored lazily in Vec<LazyField> by default for memory efficiency.
/// Fields with `inline` are stored directly on TaskStorage (for hot-path access).
///
/// Note: This struct is only used as a schema definition for the macro.
/// The macro generates `TaskStorage`, `LazyField`, and `TaskFlags` from this.
#[derive(TaskStorage)]
#[allow(
    dead_code,
    reason = "this isn't dead it is scaffolding for the derive macro"
)]
pub struct TaskStorageSchema {
    // =========================================================================
    // INLINE FIELDS (hot path, always allocated inline)
    // =========================================================================
    /// The task's aggregation number for the aggregation tree.
    /// Uses Default::default() semantics - a zero aggregation number means "not set".
    #[task_storage(
        storage = "direct",
        category = "meta",
        inline,
        default,
        variant = "AggregationNumber"
    )]
    pub aggregation_number: AggregationNumber,

    /// Tasks that depend on this task's output.
    #[task_storage(
        storage = "auto_set",
        category = "data",
        inline,
        filter_transient,
        variant = "OutputDependent",
        key_field = "task"
    )]
    pub output_dependent: AutoSet<TaskId>,

    /// The task's output value.
    /// Filtered during serialization to skip transient outputs (referencing transient tasks).
    #[task_storage(
        storage = "direct",
        category = "meta",
        inline,
        filter_transient,
        variant = "Output"
    )]
    pub output: Option<OutputValue>,

    /// Upper nodes in the aggregation tree (reference counted).
    #[task_storage(
        storage = "counter_map",
        category = "meta",
        inline,
        filter_transient,
        variant = "Upper",
        key_field = "task"
    )]
    pub upper: CounterMap<TaskId, u32>,

    // =========================================================================
    // COLLECTIBLES (meta)
    // =========================================================================
    /// Collectibles emitted by this task (reference counted).
    #[task_storage(
        storage = "counter_map",
        category = "meta",
        filter_transient,
        variant = "Collectible",
        key_field = "collectible"
    )]
    pub collectibles: CounterMap<CollectibleRef, i32>,

    /// Aggregated collectibles from the subgraph.
    #[task_storage(
        storage = "counter_map",
        category = "meta",
        filter_transient,
        variant = "AggregatedCollectible",
        key_field = "collectible"
    )]
    pub aggregated_collectibles: CounterMap<CollectibleRef, i32>,

    /// Outdated collectibles to be cleaned up (transient).
    #[task_storage(
        storage = "counter_map",
        category = "transient",
        variant = "OutdatedCollectible",
        key_field = "collectible"
    )]
    pub outdated_collectibles: CounterMap<CollectibleRef, i32>,

    // =========================================================================
    // STATE FIELDS (meta)
    // Note: Lazy direct fields use bare types - Vec presence provides optionality
    // =========================================================================
    /// Whether the task is dirty (needs re-execution).
    /// Absent = clean, present = dirty with the specified Dirtyness state.
    #[task_storage(storage = "direct", category = "meta", variant = "Dirty")]
    pub dirty: Dirtyness,

    /// Count of dirty containers in the aggregated subgraph.
    /// Absent = 0, present = actual count.
    #[task_storage(
        storage = "direct",
        category = "meta",
        variant = "AggregatedDirtyContainerCount"
    )]
    pub aggregated_dirty_container_count: i32,

    /// Individual dirty containers in the aggregated subgraph.
    #[task_storage(
        storage = "counter_map",
        category = "meta",
        filter_transient,
        variant = "AggregatedDirtyContainer",
        key_field = "task"
    )]
    pub aggregated_dirty_containers: CounterMap<TaskId, i32>,

    /// Whether clean in current session (transient flag).
    #[task_storage(
        storage = "flag",
        category = "transient",
        variant = "CurrentSessionClean"
    )]
    pub current_session_clean: bool,

    /// Count of clean containers in current session (transient).
    /// Absent = 0, present = actual count.
    #[task_storage(
        storage = "direct",
        category = "transient",
        variant = "AggregatedCurrentSessionCleanContainerCount"
    )]
    pub aggregated_current_session_clean_container_count: i32,

    /// Individual clean containers in current session (transient).
    #[task_storage(
        storage = "counter_map",
        category = "transient",
        variant = "AggregatedCurrentSessionCleanContainer",
        key_field = "task"
    )]
    pub aggregated_current_session_clean_containers: CounterMap<TaskId, i32>,

    // =========================================================================
    // FLAGS (meta) - Boolean flags stored in TaskFlags bitfield
    // Persisted flags come first, then transient flags.
    // =========================================================================
    /// Whether the task has an invalidator.
    #[task_storage(storage = "flag", category = "meta", variant = "HasInvalidator")]
    pub invalidator: bool,

    /// Whether the task output is immutable (persisted).
    #[task_storage(storage = "flag", category = "meta", variant = "Immutable")]
    pub immutable: bool,

    // =========================================================================
    // INTERNAL STATE FLAGS (transient) - Replaces InnerStorageState
    // These flags track internal state for persistence and snapshotting.
    // =========================================================================
    /// Whether meta data has been restored from persistent storage.
    #[task_storage(storage = "flag", category = "transient")]
    pub meta_restored: bool,

    /// Whether data has been restored from persistent storage.
    #[task_storage(storage = "flag", category = "transient")]
    pub data_restored: bool,

    /// Whether meta was modified before snapshot mode was entered.
    #[task_storage(storage = "flag", category = "transient")]
    pub meta_modified: bool,

    /// Whether data was modified before snapshot mode was entered.
    #[task_storage(storage = "flag", category = "transient")]
    pub data_modified: bool,

    /// Whether meta was modified after snapshot mode was entered (snapshot taken).
    #[task_storage(storage = "flag", category = "transient")]
    pub meta_snapshot: bool,

    /// Whether data was modified after snapshot mode was entered (snapshot taken).
    #[task_storage(storage = "flag", category = "transient")]
    pub data_snapshot: bool,

    /// Whether dependencies have been prefetched.
    #[task_storage(storage = "flag", category = "transient")]
    pub prefetched: bool,

    // =========================================================================
    // CHILDREN & AGGREGATION (meta)
    // =========================================================================
    /// Child tasks of this task.
    #[task_storage(
        storage = "auto_set",
        category = "meta",
        filter_transient,
        variant = "Child",
        key_field = "task"
    )]
    pub children: AutoSet<TaskId>,

    /// Follower nodes in the aggregation tree (reference counted).
    #[task_storage(
        storage = "counter_map",
        category = "meta",
        filter_transient,
        variant = "Follower",
        key_field = "task"
    )]
    pub followers: CounterMap<TaskId, u32>,

    // =========================================================================
    // DEPENDENCIES (data)
    // =========================================================================
    /// Tasks whose output this task depends on.
    #[task_storage(
        storage = "auto_set",
        category = "data",
        filter_transient,
        variant = "OutputDependency",
        key_field = "target"
    )]
    pub output_dependencies: AutoSet<TaskId>,

    /// Cells this task depends on.
    #[task_storage(
        storage = "auto_set",
        category = "data",
        filter_transient,
        variant = "CellDependency",
        key_field = "target"
    )]
    pub cell_dependencies: AutoSet<CellRef>,

    /// Collectibles this task depends on.
    #[task_storage(
        storage = "auto_set",
        category = "data",
        filter_transient,
        variant = "CollectiblesDependency",
        key_field = "target"
    )]
    pub collectibles_dependencies: AutoSet<CollectiblesRef>,

    /// Outdated output dependencies to be cleaned up (transient).
    #[task_storage(
        storage = "auto_set",
        category = "transient",
        variant = "OutdatedOutputDependency",
        key_field = "target"
    )]
    pub outdated_output_dependencies: AutoSet<TaskId>,

    /// Outdated cell dependencies to be cleaned up (transient).
    #[task_storage(
        storage = "auto_set",
        category = "transient",
        variant = "OutdatedCellDependency",
        key_field = "target"
    )]
    pub outdated_cell_dependencies: AutoSet<CellRef>,

    /// Outdated collectibles dependencies to be cleaned up (transient).
    #[task_storage(
        storage = "auto_set",
        category = "transient",
        variant = "OutdatedCollectiblesDependency",
        key_field = "target"
    )]
    pub outdated_collectibles_dependencies: AutoSet<CollectiblesRef>,

    // =========================================================================
    // DEPENDENTS - Tasks that depend on this task's cells
    // =========================================================================
    /// Tasks that depend on specific cells of this task.
    /// Maps CellId -> Set<TaskId>
    /// AutoMultimap automatically filters transient values from inner sets during encoding.
    #[task_storage(
        storage = "auto_multimap",
        category = "data",
        variant = "CellDependent",
        key_fields = "cell, task"
    )]
    pub cell_dependents: AutoMultimap<CellId, TaskId>,

    /// Tasks that depend on collectibles of a specific type from this task.
    /// Maps TraitTypeId -> Set<TaskId>
    /// AutoMultimap automatically filters transient values from inner sets during encoding.
    #[task_storage(
        storage = "auto_multimap",
        category = "meta",
        variant = "CollectiblesDependent",
        key_fields = "collectible_type, task"
    )]
    pub collectibles_dependents: AutoMultimap<TraitTypeId, TaskId>,

    // =========================================================================
    // CELL DATA (data)
    // =========================================================================
    /// Persistent cell data (serializable).
    #[task_storage(
        storage = "auto_map",
        category = "data",
        variant = "CellData",
        key_field = "cell"
    )]
    pub cell_data: AutoMap<CellId, TypedSharedReference>,

    /// Transient cell data (not serializable).
    #[task_storage(
        storage = "auto_map",
        category = "transient",
        variant = "TransientCellData",
        key_field = "cell"
    )]
    pub transient_cell_data: AutoMap<CellId, SharedReference>,

    /// Maximum cell index per cell type.
    #[task_storage(
        storage = "auto_map",
        category = "data",
        variant = "CellTypeMaxIndex",
        key_field = "cell_type"
    )]
    pub cell_type_max_index: AutoMap<ValueTypeId, u32>,

    // =========================================================================
    // TRANSIENT EXECUTION STATE (transient)
    // =========================================================================
    /// Activeness state for root/once tasks (transient).
    /// Note: Lazy storage provides natural optionality -
    /// presence in Vec<LazyField> = Some, absence = None. No Option wrapper needed.
    #[task_storage(storage = "direct", category = "transient", variant = "Activeness")]
    pub activeness: ActivenessState,

    /// In-progress execution state (transient).
    /// Note: Lazy storage provides natural optionality -
    /// presence in Vec<LazyField> = Some, absence = None. No Option wrapper needed.
    #[task_storage(storage = "direct", category = "transient", variant = "InProgress")]
    pub in_progress: InProgressState,

    /// In-progress cell state for cells being computed (transient).
    #[task_storage(
        storage = "auto_map",
        category = "transient",
        variant = "InProgressCell",
        key_field = "cell"
    )]
    pub in_progress_cells: AutoMap<CellId, InProgressCellState>,
}

// =============================================================================
// TaskFlags helper methods (for InnerStorageState compatibility)
// =============================================================================

use crate::backend::TaskDataCategory;

impl TaskFlags {
    /// Set restored flags based on category
    pub fn set_restored(&mut self, category: TaskDataCategory) {
        match category {
            TaskDataCategory::Meta => {
                self.set_meta_restored(true);
            }
            TaskDataCategory::Data => {
                self.set_data_restored(true);
            }
            TaskDataCategory::All => {
                self.set_meta_restored(true);
                self.set_data_restored(true);
            }
        }
    }

    /// Check if category is restored
    pub fn is_restored(&self, category: TaskDataCategory) -> bool {
        match category {
            TaskDataCategory::Meta => self.meta_restored(),
            TaskDataCategory::Data => self.data_restored(),
            TaskDataCategory::All => self.meta_restored() && self.data_restored(),
        }
    }

    /// Check if any snapshot flag is set
    pub fn any_snapshot(&self) -> bool {
        self.meta_snapshot() || self.data_snapshot()
    }

    /// Check if any modified flag is set
    pub fn any_modified(&self) -> bool {
        self.meta_modified() || self.data_modified()
    }
}

// =============================================================================
// TaskStorage helper methods
// =============================================================================

impl TaskStorage {
    /// Returns a reference to the flags (for state tracking like InnerStorageState)
    pub fn state(&self) -> &TaskFlags {
        &self.flags
    }

    /// Returns a mutable reference to the flags (for state tracking like InnerStorageState)
    pub fn state_mut(&mut self) -> &mut TaskFlags {
        &mut self.flags
    }

    /// Find a lazy field by predicate (immutable).
    ///
    /// The `extract` closure should return `Some(&T)` for the matching variant,
    /// or `None` for non-matching variants.
    fn find_lazy<T>(&self, extract: impl Fn(&LazyField) -> Option<&T>) -> Option<&T> {
        self.lazy.iter().find_map(extract)
    }

    /// Find a lazy field by predicate (mutable).
    ///
    /// The `extract` closure should return `Some(&mut T)` for the matching variant,
    /// or `None` for non-matching variants.
    pub fn find_lazy_mut<T>(
        &mut self,
        extract: impl Fn(&mut LazyField) -> Option<&mut T>,
    ) -> Option<&mut T> {
        self.lazy.iter_mut().find_map(extract)
    }

    /// Get or create a lazy field, returning a mutable reference.
    ///
    /// Uses a single `extract` closure that serves as both the matcher (by returning Some/None)
    /// and the value extractor. The closure is first used to find the field position,
    /// then to extract the mutable reference.
    ///
    /// # Example
    /// ```ignore
    /// let deps = storage.get_or_create_lazy(
    ///     |f| match f {
    ///         LazyField::OutputDependencies(v) => Some(v),
    ///         _ => None,
    ///     },
    ///     || LazyField::OutputDependencies(Default::default()),
    /// );
    /// ```
    fn get_or_create_lazy<T>(
        &mut self,
        extract: impl for<'a> Fn(&'a mut LazyField) -> Option<&'a mut T>,
        create: impl FnOnce() -> LazyField,
    ) -> &mut T {
        // Find the index of matching field
        let idx = self.lazy.iter_mut().position(|f| extract(f).is_some());
        if let Some(idx) = idx {
            extract(&mut self.lazy[idx]).unwrap()
        } else {
            self.lazy.push(create());
            extract(self.lazy.last_mut().unwrap()).unwrap()
        }
    }
}

// =============================================================================
// CounterMap Extension Trait
// =============================================================================

use std::{
    collections::hash_map::Entry,
    hash::Hash,
    ops::{Add, AddAssign, Sub},
};

/// Extension trait for counter map operations.
///
/// Provides common operations for maps that track reference counts, where
/// entries are automatically removed when their count reaches zero.
pub trait CounterMapExt<K, V> {
    /// Update a counter by the given delta, returning `true` if the count
    /// crossed zero (became zero or became non-zero).
    ///
    /// This is useful for tracking state transitions where crossing zero
    /// indicates a significant change (e.g., first reference added or last
    /// reference removed).
    fn update_count(&mut self, key: K, delta: V) -> bool;

    /// Update a counter by the given delta and return the new value.
    fn update_and_get(&mut self, key: K, delta: V) -> V;

    /// Update a counter using a closure that receives the current value
    /// (or None if not present) and returns the new value (or None to remove).
    fn update_with<F>(&mut self, key: K, f: F)
    where
        F: FnOnce(Option<V>) -> Option<V>;

    /// Add a new entry, panicking if the entry already exists.
    fn add_entry(&mut self, key: K, value: V);

    /// Update a signed counter by the given delta, returning `true` if the count
    /// crossed the positive boundary (became positive or became non-positive).
    ///
    /// This is useful for tracking collectibles where positive counts indicate
    /// presence and non-positive counts indicate absence.
    fn update_positive_crossing(&mut self, key: K, delta: V) -> bool;
}

/// Trait for counter value types that support the required operations.
pub trait CounterValue:
    Copy + Default + PartialEq + PartialOrd + Add<Output = Self> + AddAssign + Sub<Output = Self>
{
    /// Check if this value is zero.
    fn is_zero(&self) -> bool;

    /// Check if this value is positive (> 0).
    fn is_positive(&self) -> bool;
}

impl CounterValue for u32 {
    fn is_zero(&self) -> bool {
        *self == 0
    }

    fn is_positive(&self) -> bool {
        *self > 0
    }
}

impl CounterValue for i32 {
    fn is_zero(&self) -> bool {
        *self == 0
    }

    fn is_positive(&self) -> bool {
        *self > 0
    }
}

impl<K: Hash + Eq, V: CounterValue> CounterMapExt<K, V> for CounterMap<K, V> {
    fn update_count(&mut self, key: K, delta: V) -> bool {
        match self.entry(key) {
            Entry::Occupied(mut e) => {
                let old = *e.get();
                let new = old + delta;
                let state_change =
                    (old.is_zero() && !new.is_zero()) || (!old.is_zero() && new.is_zero());
                if new.is_zero() {
                    e.remove();
                } else {
                    *e.get_mut() = new;
                }
                state_change
            }
            Entry::Vacant(e) => {
                if !delta.is_zero() {
                    e.insert(delta);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn update_and_get(&mut self, key: K, delta: V) -> V {
        match self.entry(key) {
            Entry::Occupied(mut e) => {
                let new_value = *e.get() + delta;
                if new_value.is_zero() {
                    e.remove();
                } else {
                    *e.get_mut() = new_value;
                }
                new_value
            }
            Entry::Vacant(e) => {
                if !delta.is_zero() {
                    e.insert(delta);
                }
                delta
            }
        }
    }

    fn update_with<F>(&mut self, key: K, f: F)
    where
        F: FnOnce(Option<V>) -> Option<V>,
    {
        match self.entry(key) {
            Entry::Occupied(mut e) => match f(Some(*e.get())) {
                Some(new) => {
                    *e.get_mut() = new;
                }
                None => {
                    e.remove();
                }
            },
            Entry::Vacant(e) => {
                if let Some(new) = f(None) {
                    e.insert(new);
                }
            }
        }
    }

    fn add_entry(&mut self, key: K, value: V) {
        let old = self.insert(key, value);
        assert!(old.is_none(), "Entry already exists");
    }

    fn update_positive_crossing(&mut self, key: K, delta: V) -> bool {
        match self.entry(key) {
            Entry::Occupied(mut e) => {
                let old = *e.get();
                let new = old + delta;
                let state_change = (!old.is_positive() && new.is_positive())
                    || (old.is_positive() && !new.is_positive());
                if new.is_zero() {
                    e.remove();
                } else {
                    *e.get_mut() = new;
                }
                state_change
            }
            Entry::Vacant(e) => {
                if !delta.is_zero() {
                    e.insert(delta);
                    delta.is_positive()
                } else {
                    false
                }
            }
        }
    }
}

// =============================================================================
// CachedDataItem Adapter Extension Methods
// =============================================================================
//
// These methods provide backward compatibility with the CachedDataItem API
// while the codebase migrates to typed accessors. The adapter layer is
// intentionally kept simple - performance-critical code should use the typed
// accessor methods directly (e.g., `task.set_output(value)` instead of
// `task.insert(CachedDataItem::Output { value })`).
//
// ## Performance Notes
//
// Some adapter methods have suboptimal performance due to the enum-based API:
//
// - `add()`: Two lookups when key doesn't exist (contains_key + insert_kv). This matches the old
//   Storage::add semantics.
//
// - `update()`: Up to 3 lookups (remove + contains_key + insert_kv) instead of 1 with entry API.
//   Typed accessors use single-lookup patterns.
//
// - `get_mut_or_insert_with()`: 2-3 lookups + key clone instead of 1 lookup with entry API.
//
// - `extract_if()`: Collects keys first, then removes one by one. O(n) allocations + O(n) lookups.
//
// These inefficiencies are acceptable for this compatibility layer. The next
// PR will migrate callers to typed accessors, eliminating this overhead.
// =============================================================================

/// Extension trait for CachedDataItem adapter methods.
///
/// This trait provides simple wrapper methods that delegate to the generated
/// match-arm methods. Separating these improves code readability by keeping
/// the macro-generated code focused on the type-dispatching match arms.
pub trait CachedDataItemAdapterExt: CachedDataItemAdapter {
    /// Add a CachedDataItem to storage.
    ///
    /// Returns `true` if the item was newly added, `false` if it already existed.
    /// Does NOT overwrite if the key already exists.
    ///
    /// Note: This performs two lookups when the key doesn't exist (contains_key + insert_kv).
    /// For better performance, use typed accessors directly.
    fn add(&mut self, item: crate::data::CachedDataItem) -> bool {
        use turbo_tasks::KeyValuePair;
        let (key, value) = item.into_key_and_value();
        // Check first - add should not overwrite existing values
        if self.contains_key(&key) {
            return false;
        }
        self.insert_kv(key, value);
        true
    }

    /// Insert a CachedDataItem, returning the old value if present.
    fn insert(
        &mut self,
        item: crate::data::CachedDataItem,
    ) -> Option<crate::data::CachedDataItemValue> {
        use turbo_tasks::KeyValuePair;
        let (key, value) = item.into_key_and_value();
        self.insert_kv(key, value)
    }

    /// Check if a key exists in storage.
    fn contains_key(&self, key: &crate::data::CachedDataItemKey) -> bool {
        self.get(key).is_some()
    }

    /// Update a value in-place, creating it if it doesn't exist.
    ///
    /// Note: This performs up to 3 lookups (remove + contains_key + insert_kv).
    /// For better performance, use typed accessors with entry-style APIs.
    fn update(
        &mut self,
        key: crate::data::CachedDataItemKey,
        update: impl FnOnce(
            Option<crate::data::CachedDataItemValue>,
        ) -> Option<crate::data::CachedDataItemValue>,
    ) {
        use turbo_tasks::KeyValuePair;
        let old_value = self.remove(&key);
        if let Some(new_value) = update(old_value) {
            let item = crate::data::CachedDataItem::from_key_and_value(key, new_value);
            self.add(item);
        }
    }

    /// Get a mutable reference or insert a value created by the given closure.
    ///
    /// Note: This performs 2-3 lookups + key clone instead of 1 lookup with entry API.
    /// For better performance, use typed accessors directly.
    fn get_mut_or_insert_with(
        &mut self,
        key: crate::data::CachedDataItemKey,
        insert: impl FnOnce() -> crate::data::CachedDataItemValue,
    ) -> crate::data::CachedDataItemValueRefMut<'_> {
        if self.get(&key).is_none() {
            let value = insert();
            self.insert_kv(key.clone(), value);
        }
        self.get_mut(&key).expect("just inserted")
    }

    /// Extend storage with items from an iterator.
    /// Returns `true` if all items were newly added, `false` if any already existed.
    fn extend(
        &mut self,
        _ty: crate::data::CachedDataItemType,
        items: impl IntoIterator<Item = crate::data::CachedDataItem>,
    ) -> bool {
        let mut all_new = true;
        for item in items {
            if !self.add(item) {
                all_new = false;
            }
        }
        all_new
    }

    /// Remove items matching a predicate.
    ///
    /// Note: This collects keys first, then removes one by one - O(n) allocations + O(n) lookups.
    /// For better performance, use typed accessors with extract_if on the underlying collection.
    fn extract_if<'a, F>(
        &'a mut self,
        ty: crate::data::CachedDataItemType,
        mut predicate: F,
    ) -> Vec<crate::data::CachedDataItem>
    where
        F: for<'b> FnMut(
                crate::data::CachedDataItemKey,
                crate::data::CachedDataItemValueRef<'b>,
            ) -> bool
            + 'a,
    {
        use turbo_tasks::KeyValuePair;
        // Collect keys to remove (can't mutate while iterating)
        let keys_to_remove: Vec<_> = self
            .iter(ty)
            .filter_map(|(key, value_ref)| {
                if predicate(key.clone(), value_ref) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();

        // Remove and collect matching items
        keys_to_remove
            .into_iter()
            .filter_map(|key| {
                self.remove(&key)
                    .map(|value| crate::data::CachedDataItem::from_key_and_value(key, value))
            })
            .collect()
    }
}

/// Auto-implement for all types that implement CachedDataItemAdapter.
impl<T: CachedDataItemAdapter> CachedDataItemAdapterExt for T {}

/// Core trait for CachedDataItem adapter methods with generated match arms.
///
/// This trait is implemented by the TaskStorage derive macro and contains the
/// type-dispatching match arms that route to typed accessors. The simpler
/// wrapper methods are provided by `CachedDataItemAdapterExt`.
pub trait CachedDataItemAdapter {
    /// Insert a key-value pair, returning the old value if present.
    fn insert_kv(
        &mut self,
        key: crate::data::CachedDataItemKey,
        value: crate::data::CachedDataItemValue,
    ) -> Option<crate::data::CachedDataItemValue>;

    /// Get a reference to a CachedDataItem value by key.
    fn get(
        &self,
        key: &crate::data::CachedDataItemKey,
    ) -> Option<crate::data::CachedDataItemValueRef<'_>>;

    /// Remove a CachedDataItem by key, returning the value if present.
    fn remove(
        &mut self,
        key: &crate::data::CachedDataItemKey,
    ) -> Option<crate::data::CachedDataItemValue>;

    /// Get a mutable reference to a CachedDataItem value by key.
    fn get_mut(
        &mut self,
        key: &crate::data::CachedDataItemKey,
    ) -> Option<crate::data::CachedDataItemValueRefMut<'_>>;

    /// Count items of a specific type.
    fn count(&self, ty: crate::data::CachedDataItemType) -> usize;

    /// Iterate over items of a specific type.
    fn iter(
        &self,
        ty: crate::data::CachedDataItemType,
    ) -> Box<
        dyn Iterator<
                Item = (
                    crate::data::CachedDataItemKey,
                    crate::data::CachedDataItemValueRef<'_>,
                ),
            > + '_,
    >;
}
