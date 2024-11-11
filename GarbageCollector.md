# Garbage Collecting Allocator for Graph Nodes

There are two systems of allocation: an arena allocator strictly for `DagNode` allocation, and a copying "bucket" allocator for raw memory ("storage") that may be used by `DagNode`s. Any memory owned by a `DagNode` must be allocated from bucket storage. Both allocators use a simple mark-sweep garbage collection algorithm.

## Arena Allocator

The arena allocator allocates pools of memory in fixed size chunks. New allocations are done via linear search. 

### Components

#### Arenas

 - An `Arena` is a fixed size array of `DagNode`s together with a pointer to the "next" arena. Arenas only store `DagNode` objects.
 - Arena's are stored in a single linked list whose head is stored in `self.first_arena` and tail is stored in `self.last_arena`. The `Allocator` manages arenas.
 - `self.current_arena` points to the current active arena (or null if there is none). 

#### `Allocator` Fields

- **`nr_arenas`**: The total number of arenas currently allocated. This keeps track of how many arenas are in use by the allocator.

- **`nr_nodes_in_use`**: The number of `DagNode`s currently in use. This helps the allocator manage memory usage and decide when garbage collection is needed.

- **`current_arena_past_active_arena`**: A flag indicating whether the current arena has surpassed the "active" arena (i.e., the arena where the most recent allocations are happening). This helps track the progress of allocation in the arena list and is used to determine when a new arena needs to be allocated.

- **`need_to_collect_garbage`**: A flag indicating whether garbage collection should be triggered. If the allocator detects that memory usage has exceeded a threshold, this flag is set to true, signaling the need to clean up unused memory.

- **`first_arena`**: A pointer to the first arena in the linked list of arenas. This is the head of the list of arenas that the allocator manages.

- **`last_arena`**: A pointer to the last arena in the linked list of arenas. This represents the tail of the list and helps efficiently append new arenas.

- **`current_arena`**: A pointer to the current active arena where new `DagNode`s are being allocated. If `current_arena` is null, it means there is no active arena, and the allocator needs to allocate a new one.

- **`next_node`**: A pointer to the next free `DagNode` in the current arena. This pointer is updated during each allocation to point to the next available node. It helps track where the next allocation should happen.

- **`end_pointer`**: A pointer to the end of the current arena, used to check whether the arena has space for more allocations. If `next_node` reaches `end_pointer`, the arena is considered full, and a new arena needs to be allocated.

- **`last_active_arena`**: A pointer to the last arena that was actively used. This is used for tracking purposes, especially when determining if the current arena has surpassed the "active" arena or when allocating new arenas.

- **`last_active_node`**: A pointer to the last `DagNode` allocated in the last active arena. This helps track where the most recent allocation took place in the last active arena, aiding in future allocations.

### Allocation Flow in `allocate_dag_node`

#### The fast allocation path

The **fast allocation path** in the `allocate_dag_node` method is used when there is available space within the current arena for a new `DagNode`. This path avoids the need for allocating new arenas or performing more complex checks. Here’s how the fast path works:

1. **Check if the current node is within the bounds of the current arena**:
    - The method starts by setting `current_node` to `self.next_node`, which points to the next available `DagNode` in the current arena.
    - The method then enters a loop over all nodes from `self.next_node` to `self.end_pointer`, which marks the end of the current arena. If `current_node` equals `self.end_pointer`, it means the current arena is full, and the method switches to the **slow allocation path**.
    - If the `current_node` can be reused, or if we hit the end and allocate a new node using the slow path, the loop breaks.
    - If the `current_node` cannot be reused, it's `MARKED` flag is cleared (in preparation for the next mark phase of the garbage collector).

2. **Node Reuse**:
    - If `node.simple_reuse()` returns `true`, it means the current `DagNode` is not marked and does not need destruction--it can be immediately reused. In this case, the method breaks out of the loop and returns `current_node`, which is the allocated (or reused) `DagNode`.

3. **Node Destruction**:
    - If the node cannot be reused, the allocator checks whether the node is marked. If the node is not marked, it indicates that the node is no longer in use, but it needs to be destroyed.
    - The `drop_in_place()` function is called on the node, which runs its destructor. After this, the loop breaks, and the allocator returns the destroyed node.

4. **Clear the "Marked" Flag**:
    - If the node is marked, it is cleared by removing the `DagNodeFlag::Marked` flag. This indicates that the node is now "inactive" again and can be reused or allocated as a valid object in the future.

5. **Move to the Next Node**:
    - After processing the current node, if , the pointer `current_node` is incremented to point to the next available node in the arena (`current_node = current_node.add(1)`).
    - The loop continues, checking the next node in the arena for reuse or destruction until a suitable node is found.

6. **Return the Allocated Node**:
    - Once a valid node is found (either reused or newly allocated), `self.next_node` is updated (to `current_node.add(1)`), and the `current_node` is returned as the newly allocated `DagNode`.
    - In all cases, every node up to and including `current_node` do not have their `DagNodeFlag::Marked` flag set.

#### The slow allocation path

The **slow allocation path** is invoked when the current arena is full (i.e., when there is no more space for new `DagNode`s) or when additional memory is needed beyond what the current arena can provide. In such cases, the allocator needs to either allocate a new arena or perform more complex management of the arenas. The `slow_new_dag_node` method handles these cases.

Here’s a detailed breakdown of how the slow allocation path works:

1. **Check for First Arena Allocation**:
    - The method first checks if `self.current_arena` is `null`. If it is `null`, it means no arena has been allocated yet, and this is the first allocation.
    - In this case, the allocator allocates a new arena by calling `self.allocate_new_arena()`.
    - Once the new arena is allocated, the first `DagNode` in the arena is retrieved by calling `arena.first_node()`. This is the starting point for node allocation in the new arena.
    - The `end_pointer` is set to the position just before the end of the arena, leaving space for potential future allocations (with some reserved space defined by `RESERVE_SIZE`).
    - Finally, the method returns the `first_node`, which is the starting point for allocations in this newly created arena.

2. **Handle Existing Arena with No Next Arena**:
    - If the current arena is not null (i.e. the first arena has been allocated), the method checks if the arena has a `next_arena`.
    - If the current arena does not have a `next_arena` (i.e., it is the last arena in the list):
        - **Trigger Garbage Collection**: The `need_to_collect_garbage` flag is set to `true` because the allocator may need to clean up memory to free space in the existing arenas.
        - **Check Space in the Current Arena**:  The last arena in the linked list is given reserve space, which might not be used yet. It compares `self.end_pointer` (the current end of the arena) with `end_node` (which is the last `DagNode` in the current arena). If the arena has space, it updates `next_node` to `end_pointer`, effectively marking the space as used. If the arena is full, a new arena needs to be allocated.
        - **Allocate New Arena**: If the current arena is full and there is no `next_arena`, the allocator allocates a new arena by calling `self.allocate_new_arena()`. This new arena becomes the new `current_arena`, and its `first_node` is returned.
        - **Reserve Space**: The `end_pointer` is updated to point to the end of the new arena (the total capacity minus a reserve amount).

3. **Handle Existing Arena with Next Arena**:
    - If there is a `next_arena` (i.e., the current arena is not the last one in the list), the method updates `self.current_arena` to point to the next arena (`arena`).
    - **Move to the Next Arena**: The `current_arena` is updated, and the `next_node` is reset to the first `DagNode` in the new arena.
    - The method then checks if the new arena has a `next_arena`. If it doesn't (`is_null()` returns `true`), it sets `end_pointer` to the position just before the reserved space in the new arena. Otherwise, it sets the `end_pointer` to the position just before the last `DagNode` in the new arena.

4. **Lazy Sweep**:
    - After ensuring that the correct arena and memory location are set up, the method enters another loop to find a free `DagNode` in the current arena. This loop is virtually identical to the logic in the fast path in `allocate_dag_node`.
    - The loop checks each `DagNode` between `self.next_node` and `self.end_pointer` to find a reusable node or a free node that can be allocated.
    - If a reusable node is found (`simple_reuse()`), it is returned immediately.
    - If the node is not marked and is not reusable, the allocator destroys it using `drop_in_place()` and returns the current node.
    - If the node is marked, the mark is cleared (`DagNodeFlag::Marked` is removed, in preparation for the next garbage collection), and the loop continues to the next node in the arena.

### Garbage Collection

The arena allocator uses a simple mark-and-sweep algorithm to reclaim unused memory. The sweep phase is "lazy": it 
is done during search for a free `DagNode` and also right before garbage collection. In particular, 
`self.collect_garbage()` immediately calls `self.sweep_arenas()`, which performs the sweep phase.

#### The Sweep Phase - Tidy Arenas

This method is responsible for efficiently managing memory in arenas by cleaning up unneeded nodes and resetting flags for those that will be reused. It accomplishes this by iterating through each arena and its associated DagNodes, clearing the “marked” flags and calling destructors (drop_in_place) for nodes that no longer serve a purpose. During this sweep, the method also tracks the last active nodes in memory, capturing two key pieces of information:

1.	new_last_active_arena: This variable points to the arena holding the final active node post-sweep, marking the boundary of currently in-use memory.
2.	new_last_active_node: This variable identifies the last active DagNode within the designated arena.

Together, these pointers are updated throughout the scanning process, enabling the method to efficiently free up memory while preserving any necessary nodes for future reuse.

Note that all nodes prior to `self.current_node` have already been swept by the allocator. The allocation functions 
guarantee that every node prior to `self.current_node` have their "marked" flag cleared and, if necessary, their 
constructor run. 

- The `sweep_arenas` method iterates over all arenas from the `current_arena` to the `last_active_arena`, cleaning up 
  marked nodes and calling destructors on unmarked nodes that need destruction.
- It updates flags and prepares nodes for reuse.
- It ensures that any nodes that need destruction are properly deallocated using `drop_in_place`, and the nodes that are still in use have their "marked" flags removed.
- The method also tracks and updates the pointers to the last active arena and node to ensure that the allocator maintains an accurate record of used memory after sweeping.

#### The Mark Phase 

After sweeping the arenas, `self.collect_garbage()` proceeds to the mark phase. 

 - `self.nr_nodes_in_use = 0;` resets the counter for the number of nodes currently in use, as it will be recalculated 
 during the garbage collection process.
 - The `mark_roots()` function marks all live nodes in memory. This is part of the mark-and-sweep garbage collection process.
- The total number of nodes across all arenas is calculated by multiplying the number of arenas (`self.nr_arenas`) by the size of each arena (`ARENA_SIZE`).
- This value represents the total potential capacity of the allocator in terms of how many nodes could be held in all arenas combined.
- A "slop factor" is computed. The slop factor represents how much additional capacity beyond the count of active 
  nodes the arena allocator should have.
- The number of new arenas required is calculated by multiplying `self.nr_nodes_in_use` by the `slop_factor`. This 
  gives an estimate of how many total nodes the allocator should be able to store. The result is then rounded up 
  using `.ceil()` to ensure that even a small shortfall in space would trigger the allocation of a full arena.
- If the current number of arenas (`self.nr_arenas`) is less than the calculated required number of new arenas (`new_arenas`), new arenas are allocated one by one using the `self.allocate_new_arena()` method until the required number of arenas is met.
- `self.current_arena_past_active_arena` is set to `false`, indicating that the current arena has not yet passed the "active" arena.
- `self.current_arena` is reset to point to the first arena in the linked list (`self.first_arena`).
- The next node to be allocated is set to `current_arena.first_node()`, which is the first available `DagNode` in the current arena.
- If the current arena is the last arena (i.e., there is no `next_arena`), the `end_pointer` is set to the position just before the reserved space (`ARENA_SIZE - RESERVE_SIZE`).
- If the arena has a `next_arena`, the `end_pointer` is set to the end of the arena (`ARENA_SIZE`), indicating where the next allocation will occur.

## Bucket Allocator

The `Allocator` struct and its associated methods manage memory allocation for raw bytes (referred to as "storage") using a system of **buckets**. This allocator is distinct from the arena-based allocator that allocates nodes; here, buckets manage storage in chunks that are allocated and used for various purposes by nodes.

The bucket allocator is a copying allocator that uses simple mark-sweep garbage collection: during the marking phase, nodes owning allocated bucket storage reallocate their needed storage (into initially unused buckets) and copy their data to the new storage. Then, the buckets that were previously in use are reset to empty and listed as available for use when a new bucket is needed.

### Components

#### Buckets
- Buckets are containers that store raw memory blocks. Each bucket has a certain amount of storage and can be used to allocate smaller chunks of memory.
- Buckets in use are linked in a list, with each bucket containing information about how much memory is free and where the next free byte is located.
- The buckets are managed by the `Allocator` struct, and they are linked in two lists: one for **in-use** buckets (`bucket_list`) and one for **unused** buckets (`unused_list`).

#### `Allocator` Fields
- `nr_buckets`: The total number of buckets that have been allocated.
- `bucket_list`: A linked list of buckets that are currently in use.
- `unused_list`: A linked list of buckets that are not currently in use, meaning they can be reused when space is needed.
- `bucket_storage`: The total amount of memory allocated across all buckets.
- `storage_in_use`: The amount of memory that is currently allocated for use from the buckets.
- `target`: The threshold beyond which the allocator will initiate garbage collection. If the total amount of memory used exceeds this target, the garbage collection flag (`need_to_collect_garbage`) is set.

### Allocation Flow in `allocate_storage`

The method `allocate_storage` is responsible for allocating memory from the bucket system. Here's what the allocator does internally when `allocate_storage` is called.

#### The fast allocation path

- The allocator updates its `storage_in_use` field by adding the `bytes_needed` amount.
- If the total storage in use exceeds the `target` value, the allocator sets the `need_to_collect_garbage` flag to `true`, indicating that garbage collection should be triggered to reclaim memory.
- The allocator searches through the `bucket_list` (which contains all the "in use" buckets) to find one with enough free space (`bucket.bytes_free >= bytes_needed`).
- If such a bucket is found, the allocation proceeds:
    - The `bytes_free` value of the bucket is reduced by `bytes_needed`.
    - The `next_free` pointer in the bucket is updated to point to the new available memory location after the allocation.
    - The function then returns the pointer to the allocated memory.
- If no suitable bucket is found (i.e., no bucket has enough free space), the allocator proceeds to allocate a new bucket by calling `slow_allocate_storage`.

#### The slow allocation path

When the `allocate_storage` method cannot find enough free space in the existing buckets, it calls `slow_allocate_storage` to allocate a new bucket. Here is what happens internally when `allocate_storage` calls  `slow_allocate_storage`.

- The allocator loops through the `unused_list` (which contains buckets that are currently not being used) for a bucket that has enough free space.
- If such a bucket is found, it is moved from the unused list to the in-use list (`bucket_list`), and memory is allocated from it as described in the previous steps.
- If no suitable unused bucket is found, a new bucket is allocated.
- The size of the new bucket is determined by multiplying `bytes_needed` by a `BUCKET_MULTIPLIER`. This ensures that the bucket is large enough to accommodate the requested memory, with a minimum bucket size (`MIN_BUCKET_SIZE`).
- A new `Bucket` is allocated using the `alloc_zeroed` function, ensuring the memory is initialized to zero.
- The new bucket is initialized by setting:
    - `nr_bytes`: the total size of the bucket.
    - `bytes_free`: the amount of free space in the bucket after the initial allocation.
    - `next_free`: the pointer to the next available memory location after allocation.
- The bucket is then added to the in-use linked list.

### Garbage Collection

Active objects that are still in use (i.e., live data) are copied to new locations, and the old locations are then freed. This pattern reduces fragmentation and improves locality for certain access patterns. Garbage collection proceeds as follows.

#### Prior to the mark phase

 - The linked list of buckets in use (initially stored in `bucket_list`) is saved in a temporary variable for later use in the sweep stage.
 - The linked list of buckets not in use (initially stored in `unused_list`) is placed in `bucket_list`. These buckets (if there are any) are all empty.
 - The `unused_list` is set to null, that is, the list is set to empty.

#### The mark phase

 - The linked list of node roots is traverse, and `mark_roots()` is called on each.
 - The DAG of nodes in use is traversed, and `mark()` is called on each.
 - Within `mark()`, any bucket storage owned by the node is reallocated, with values copied over to the newly allocated storage. 
 - Within the allocator, storage allocations during the mark phase are allocated from `self.bucket_list`, which initially only contains empty buckets. Thus, during the mark phase, only active memory is allocated.
 - If there is insufficient space in the `self.bucket_list` to accommodate the live objects during the marking phase, new buckets are allocated one at a time, and they are added to `self.bucket_list`. 
 - Buckets in `self.bucket_list` now point to either newly allocated memory or to the memory of live objects copied over from other locations. The `self.bucket_list` may grow as new buckets are added for the copied data.

#### The sweep phase

The original linked list of in-use buckets is saved at the beginning of garbage collection. After the mark phase, each bucket in this list is reset to "empty": the `bytes_free` of each bucket is reset to the full capacity of the bucket, and the memory is marked as available for future allocations. After the sweep phase, all the buckets (now in `self.unused_list`) are essentially reset to an empty state. The memory is now free, and new allocations can be made in the newly allocated buckets (`self.bucket_list`) or in the buckets that were just freed.
