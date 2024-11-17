/*!
The allocator for garbage collected memory. This is really two different allocators which collect garbage at the same time:

 1. An arena allocator exclusively for allocating `DagNode` objects. All garbage collected nodes must be allocated with this allocator.
 2. A "bucket" allocator exclusively for allocating any memory owned by `DagNode` objects. Nodes may have several arguments, which are other nodes. The arguments are stored as arrays of pointers to the argument nodes, and nodes must allocate these arrays of pointers using the bucket allocator and hold on to a pointer to the array.

See GarbageCollector.md for a detailed explanation of how it works. Below is a brief summary of how each one works..

# Arena Allocator

The arena allocator manages memory by organizing it into arenas, which are fixed size arrays of nodes available for allocation. The allocator uses a simple mark-and-sweep algorithm to collect garbage, but the sweep phase is "lazy." When the program requests a new node allocation, the allocator searches linearly for free nodes within these arenas and reuses them when possible. During this linear search, the allocator performs a "lazy sweep," clearing all "marked" flags on nodes and running destructors when necessary. This proceeds until either an available node is found and returned or all nodes are found to be in use, in which case it may expand by creating a new arena or adding capacity to existing ones.

When garbage collection is triggered, the allocator then sweeps the remaining (not yet searched) part of the arena(s). Then it begins the mark phase. During marking, the allocator requests all node roots to flag nodes that are actively in use so that they’re preserved. During this phase, the number of active nodes is computed. After marking, the allocator compares it's total node capacity to the number of active nodes and, if the available capacity is less than a certain "slop factor," more arenas are allocated from system memory. The "cursor" for the linear search is then reset to the first node of the first arena.

Since the sweep phase is done lazily, the time it takes to sweep the arenas is amortized between garbage collection events. Because garbage collection is triggered when the linear search for free nodes nears the end of the last arena, allocating a "slop factor" of extra arenas keeps garbage collection events low.

# Bucket Allocator

The Bucket allocator manages memory by organizing it into buckets, each containing raw memory that can be allocated in smaller chunks. When a program requests memory, the allocator first searches the in-use buckets for a free chunk. In the typical case, the current active bucket has the capacity to allocate the requested chunk, and so the allocator acts as a "bump" allocator. If no suitable space is found, it checks unused buckets (if any exist) or allocates new ones to accommodate the request.

The garbage collection process in the bucket allocator follows a mark-and-sweep pattern with a copying strategy. During the mark phase, the allocator traverses the live data and copies it to available initially empty buckets (i.e. buckets which were empty prior to garbage collection). If the available buckets do not have enough space to accommodate the live objects, new buckets are allocated and added to the list. Once the objects are copied, the old memory locations are free to be collected in the sweep phase.

In the sweep phase, the allocator clears the old buckets, resetting their free space to the full bucket size. These buckets are then moved to the unused list and reset to an empty state, making them available for future allocations.

Because live objects are relocated during garbage collection to previously empty buckets, there is no fragmentation after garbage collection. What's more, copying occurs in depth-first order on the graph nodes, improving locality for certain access patterns.

*/

use std::{
  alloc::{alloc_zeroed, Layout},
  cmp::max,
  ptr::drop_in_place,
  sync::Mutex
};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::MutexGuard;
use once_cell::sync::Lazy;
use crate::dag_node::{
  arena::Arena,
  bucket::Bucket,
  flags::DagNodeFlag,
  flags::DagNodeFlags,
  node::DagNode,
  root_container::mark_roots,
  Void
};

// Constant Allocator Parameters
const SMALL_MODEL_SLOP: f64   = 8.0;
const BIG_MODEL_SLOP  : f64   = 2.0;
const LOWER_BOUND     : usize =  4 * 1024 * 1024; // Use small model if <= 4 million nodes
const UPPER_BOUND     : usize = 32 * 1024 * 1024; // Use big model if >= 32 million nodes
// It looks like Maude assumes DagNodes are 6 words in size, but ours are 3 words,
// at least so far.
pub(crate) const ARENA_SIZE: usize = 5460;           // Arena size in nodes; 5460 * 6 + 1 + new/malloc_overhead <= 32768 words
const RESERVE_SIZE         : usize = 256;            // If fewer nodes left call GC when allowed
const BUCKET_MULTIPLIER    : usize = 8;              // To determine bucket size for huge allocations
const MIN_BUCKET_SIZE      : usize = 256 * 1024 - 8; // Bucket size for normal allocations
const INITIAL_TARGET       : usize = 220 * 1024;     // Just under 8/9 of MIN_BUCKET_SIZE
const TARGET_MULTIPLIER    : usize = 8;

static ACTIVE_NODE_COUNT: AtomicUsize = AtomicUsize::new(0);

static GLOBAL_NODE_ALLOCATOR: Lazy<Mutex<Allocator>> = Lazy::new(|| {
  Mutex::new(Allocator::new())
});

#[inline(always)]
pub fn acquire_allocator() -> MutexGuard<'static, Allocator> {
  match GLOBAL_NODE_ALLOCATOR.try_lock() {
    Ok(allocator) => allocator,
    Err(e) => {
      panic!("Global allocator is deadlocked: {}", e);
    }
  }
  // GLOBAL_NODE_ALLOCATOR.lock().unwrap()
}
#[inline(always)]
pub fn increment_active_node_count() {
  ACTIVE_NODE_COUNT.fetch_add(1, Relaxed);
}
#[inline(always)]
pub fn active_node_count() {
  ACTIVE_NODE_COUNT.load(Relaxed);
}

pub struct Allocator{
  // General settings
  show_gc   : bool, // Do we report GC stats to user
  early_quit: u64,  // Do we quit early for profiling purposes

  // Arena management variables
  nr_arenas                      : u32,
  current_arena_past_active_arena: bool,
  need_to_collect_garbage        : bool,
  first_arena                    : *mut Arena,
  last_arena                     : *mut Arena,
  current_arena                  : *mut Arena,
  next_node                      : *mut DagNode,
  end_pointer                    : *mut DagNode,
  last_active_arena              : *mut Arena,
  last_active_node               : *mut DagNode,

  // Bucket management variables
  nr_buckets    : u32,    // Total number of buckets
  bucket_list   : *mut Bucket, // Linked list of "in use" buckets
  unused_list   : *mut Bucket, // Linked list of unused buckets
  bucket_storage: usize,  // Total amount of bucket storage (bytes)
  storage_in_use: usize,  // Amount of bucket storage in use (bytes)
  target        : usize,  // Amount to use before GC (bytes)
}

// Access is hidden behind a mutex.
unsafe impl Send for Allocator {}
// unsafe impl Sync for Allocator {}

impl Allocator {
  pub fn new() -> Self {
    Allocator {
      show_gc          : true,
      early_quit       : 0,
      nr_arenas        : 0,

      current_arena_past_active_arena: false,
      need_to_collect_garbage        : false,

      first_arena      : std::ptr::null_mut(),
      last_arena       : std::ptr::null_mut(),
      current_arena    : std::ptr::null_mut(),
      next_node        : std::ptr::null_mut(),
      end_pointer      : std::ptr::null_mut(),
      last_active_arena: std::ptr::null_mut(),
      last_active_node : std::ptr::null_mut(),

      nr_buckets    : 0,
      bucket_list   : std::ptr::null_mut(),
      unused_list   : std::ptr::null_mut(),
      bucket_storage: 0,
      storage_in_use: 0,
      target        : INITIAL_TARGET,
    }
  }

  /// Tell the garbage collect to collect garbage if it needs to.
  /// You can query whether it needs to by calling `want_to_collect_garbage`,
  /// but this isn't necessary.
  #[inline(always)]
  pub fn ok_to_collect_garbage(&mut self) {
    if self.need_to_collect_garbage {
      unsafe{ self.collect_garbage(); }
    }
  }

  /// Query whether the allocator has any garbage to collect.
  #[inline(always)]
  pub fn want_to_collect_garbage(&self) -> bool {
    self.need_to_collect_garbage
  }

  /// Allocates the given number of bytes using bucket storage.
  pub fn allocate_storage(&mut self, bytes_needed: usize) -> *mut Void {
    assert_eq!(bytes_needed % size_of::<usize>(), 0, "only whole machine words can be allocated");
    self.storage_in_use += bytes_needed;

    if self.storage_in_use > self.target {
      self.need_to_collect_garbage = true;
    }

    let mut b = unsafe { self.bucket_list.as_mut() };

    while let Some(bucket) = b {
      if bucket.bytes_free >= bytes_needed {
        bucket.bytes_free -= bytes_needed;
        let t = bucket.next_free;
        bucket.next_free = {
          // align next_free on 8 byte boundary
          let mut next_free = unsafe { t.add(bytes_needed) };
          let align_offset = next_free.align_offset(8);
          if align_offset == usize::MAX {
            panic!("Cannot align memory to 8 byte boundary")
          }
          bucket.bytes_free -= align_offset;
          next_free = unsafe { next_free.add(align_offset) };

          next_free
        };

        return t;
      }

      b = unsafe { bucket.next_bucket.as_mut() };
    }

    // No space in any bucket, so we need to allocate a new one.
    unsafe{ self.slow_allocate_storage(bytes_needed) }
  }

  /// Allocates a new `DagNode`
  pub fn allocate_dag_node(&mut self) -> *mut DagNode {
    // ToDo: I think we can replace these pointers with indices into the current arena's data array.
    //       Includes next_node, end_pointer, end_node.
    let mut current_node = self.next_node;

    unsafe{
      loop {
        if current_node == self.end_pointer {
          // Arena is full. Allocate a new one.
          current_node = self.slow_new_dag_node();
          break;
        }

        { // Scope of `current_node_mut: &mut DagNode`
          let current_node_mut = current_node.as_mut_unchecked();
          if current_node_mut.simple_reuse() {
            break;
          }
          if !current_node_mut.is_marked() {
            // Not marked, but needs destruction because it's not simple reuse.
            drop_in_place(current_node_mut);
            break;
          }
          current_node_mut.flags.remove(DagNodeFlag::Marked);
        }

        current_node = current_node.add(1);
      }

      self.next_node = current_node.add(1);
    } // end of unsafe block

    increment_active_node_count();
    current_node
  }


  /// Allocates a new arena, adding it to the linked list of arenas, and
  /// returns (a pointer to) the new arena.
  unsafe fn allocate_new_arena(&mut self) -> *mut Arena {
    #[cfg(feature = "gc_debug")]
    {
      eprintln!("allocate_new_arena()");
      self.dump_memory_variables();
    }

    let arena = Arena::allocate_new_arena();
    match self.last_arena.as_mut() {
      None => {
        // Allocating the first arena
        self.first_arena = arena;
      }
      Some(last_arena) => {
        last_arena.next_arena = arena;
      }
    }

    self.last_arena = arena;
    self.nr_arenas += 1;

    arena
  }

  /// Allocate a new `DagNode` when the current arena is (almost) full.
  unsafe fn slow_new_dag_node(&mut self) -> *mut DagNode {
    #[cfg(feature = "gc_debug")]
    {
      eprintln!("slow_new_dag_node()");
      self.dump_memory_variables();
    }

    loop {
      if self.current_arena.is_null() {
        // Allocate the first arena
        self.current_arena = self.allocate_new_arena();
        let arena          = self.current_arena.as_mut_unchecked();
        let first_node     = arena.first_node();
        // The last arena in the linked list is given a reserve.
        self.end_pointer   = first_node.add(ARENA_SIZE - RESERVE_SIZE);

        return first_node;
      }

      // Checked for null above.
      let current_arena = self.current_arena.as_mut_unchecked();
      let arena         = current_arena.next_arena;

      if arena.is_null() {
        self.need_to_collect_garbage = true;
        let end_node = current_arena.first_node().add(ARENA_SIZE);

        if self.end_pointer != end_node {
          // Use up the reserve
          self.next_node   = self.end_pointer; // Next node is invalid where we are called.
          self.end_pointer = end_node;
        } else {
          // Allocate a new arena
          if self.current_arena == self.last_active_arena {
            self.current_arena_past_active_arena = true;
          }

          self.current_arena = self.allocate_new_arena();
          let arena          = self.current_arena.as_mut_unchecked();
          let first_node     = arena.first_node();
          self.end_pointer   = first_node.add(ARENA_SIZE); // ToDo: Why no reserve here?

          return first_node;
        }
      } // end if arena.is_null()
      else {
        // Use next arena
        if self.current_arena == self.last_active_arena {
          self.current_arena_past_active_arena = true;
        }

        self.current_arena = arena;
        let current_arena  = arena.as_mut_unchecked();
        self.next_node     = current_arena.first_node();

        match current_arena.next_arena.is_null() {
          true => {
            // The last arena in the linked list is given a reserve.
            self.end_pointer = self.next_node.add(ARENA_SIZE - RESERVE_SIZE);
          }
          false => {
            self.end_pointer = self.next_node.add(ARENA_SIZE);
          }
        }
      }

      #[cfg(feature = "gc_debug")]
      self.check_invariant();

      // Now execute lazy sweep to actually find a free location. Note that this is the same code as in
      // `allocate_dag_node`, except there is no `slow_new_dag_node` case.

      let end_node   = self.end_pointer;
      let mut cursor = self.next_node;
      // Loop over all nodes from self.next_node to self.end_pointer
      loop{
        if cursor == end_node {
          // We've reached the end of the arena without finding a free location.
          // Try everything again.
          break;
        }

        let cursor_mut = cursor.as_mut_unchecked();

        if cursor_mut.simple_reuse(){
          return cursor;
        }
        if !cursor_mut.is_marked() {
          drop_in_place(cursor_mut);
          return cursor;
        }

        cursor_mut.flags.remove(DagNodeFlag::Marked);

        cursor = cursor.add(1);
      } // end loop over all nodes
    } // end outermost loop
  }

  /// Allocates the given number of bytes by creating more bucket storage.
  unsafe fn slow_allocate_storage(&mut self, bytes_needed: usize) -> *mut u8 {
    #[cfg(feature = "gc_debug")]
    {
      eprintln!("slow_allocate_storage()");
    }
    // Loop through the bucket list
    let mut prev_bucket: *mut Bucket = std::ptr::null_mut();
    let mut bucket:      *mut Bucket = self.unused_list;
    loop{
      if bucket.is_null() {
        break;
      }
      let bucket_mut = bucket.as_mut_unchecked();
      if bucket_mut.bytes_free >= bytes_needed {
        // Move bucket from unused list to in use list
        if prev_bucket.is_null() {
          self.unused_list = bucket_mut.next_bucket;
        } else {
          prev_bucket.as_mut_unchecked().next_bucket = bucket_mut.next_bucket;
        }

        bucket_mut.next_bucket = self.bucket_list;
        self.bucket_list = bucket;

        // Allocate storage from bucket
        bucket_mut.bytes_free -= bytes_needed;
        let t = bucket_mut.next_free;
        bucket_mut.next_free = t.add(bytes_needed);
        return t;
      }

      prev_bucket = bucket;
      bucket = bucket_mut.next_bucket
    }

    // Create a new bucket.
    // ToDo: This should be a static method on Bucket.
    let mut size = BUCKET_MULTIPLIER * bytes_needed;
    size = size.max(MIN_BUCKET_SIZE);

    bucket = {
      let chunk: *mut u8 = alloc_zeroed(
        Layout::from_size_align(size, 8).unwrap()
      );
      chunk as *mut Bucket
    };

    self.nr_buckets        += 1;
    let t: *mut Void        = bucket.add(1) as *mut Void;
    let byte_count          = size - size_of::<Bucket>();

    self.bucket_storage    += byte_count;
    // Initialize the bucket
    let bucket_mut          = bucket.as_mut_unchecked();
    bucket_mut.nr_bytes     = byte_count;
    bucket_mut.bytes_free   = byte_count - bytes_needed;
    bucket_mut.next_free    = t.add(bytes_needed);
    // Put it at the head of the bucket linked list
    bucket_mut.next_bucket  = self.bucket_list;
    self.bucket_list        = bucket;

    t
  }

  unsafe fn collect_garbage(&mut self) {
    static mut GC_COUNT: u64 = 0;

    if self.first_arena.is_null() {
      return;
    }

    self.sweep_arenas();
    #[cfg(feature = "gc_debug")]
    self.check_arenas();

    // Mark phase
    
    let old_active_node_count = ACTIVE_NODE_COUNT.load(Relaxed);
    ACTIVE_NODE_COUNT.store(0, Relaxed); // to be updated during mark phase.
    
    // Prep bucket storage for sweep
    let old_storage_in_use = self.storage_in_use;
    let bucket             = self.bucket_list;
    self.bucket_list       = self.unused_list;
    self.unused_list       = std::ptr::null_mut();
    self.storage_in_use    = 0;

    mark_roots();

    // Garbage Collection for Buckets

    self.unused_list = bucket;
    while !bucket.is_null() {
      let bucket_mut        = bucket.as_mut_unchecked();
      bucket_mut.bytes_free = bucket_mut.nr_bytes;
      bucket_mut.next_free  = bucket.add(1) as *mut Void;
    }
    self.target = max(self.target, TARGET_MULTIPLIER*self.storage_in_use);

    // Garbage Collection for Arenas

    let active_node_count = ACTIVE_NODE_COUNT.load(Relaxed); // updated during mark phase
    
    // Calculate if we should allocate more arenas to avoid an early gc.
    let node_count = (self.nr_arenas as usize) * ARENA_SIZE;
    GC_COUNT += 1;
    let gc_count = GC_COUNT; // To silence shared_mut_ref warning

    if self.show_gc {
      println!("Collection: {}", gc_count);

      println!(
        "Arenas: {}\tNodes: {} ({:.2} MB)\tCollected: {} ({:.2}) MB\tNow: {} ({:.2} MB)",
        self.nr_arenas,
        node_count,
        ((node_count * size_of::<DagNode>()) as f64) / (1024.0 * 1024.0),
        old_active_node_count - active_node_count,
        (((old_active_node_count - active_node_count) * size_of::<DagNode>()) as f64) / (1024.0 * 1024.0),
        active_node_count,
        ((active_node_count * size_of::<DagNode>()) as f64) / (1024.0 * 1024.0),
      );

      println!(
        "Buckets: {}\tBytes: {} ({:.2} MB)\tIn use: {} ({:.2} MB)\tCollected: {} ({:.2} MB)\tNow: {} ({:.2} MB)",
        self.nr_buckets,
        self.bucket_storage,
        (self.bucket_storage as f64) / (1024.0 * 1024.0),
        old_storage_in_use,
        (old_storage_in_use as f64) / (1024.0 * 1024.0),
        old_storage_in_use - self.storage_in_use,
        ((old_storage_in_use - self.storage_in_use) as f64) / (1024.0 * 1024.0),
        self.storage_in_use,
        (self.storage_in_use as f64) / (1024.0 * 1024.0),
      );
    }

    if GC_COUNT == self.early_quit{
      std::process::exit(0);
    }

    // Compute slop factor
    // Case: ACTIVE_NODE_COUNT >= UPPER_BOUND
    let mut slop_factor: f64 = BIG_MODEL_SLOP;
    if ACTIVE_NODE_COUNT.load(Relaxed) < LOWER_BOUND {
      // Case: ACTIVE_NODE_COUNT < LOWER_BOUND
      slop_factor = SMALL_MODEL_SLOP;
    } else if ACTIVE_NODE_COUNT.load(Relaxed) < UPPER_BOUND {
      // Case: LOWER_BOUND <= ACTIVE_NODE_COUNT < UPPER_BOUND
      // Linearly interpolate between the two models.
      slop_factor += ((UPPER_BOUND - active_node_count) as f64 * (SMALL_MODEL_SLOP - BIG_MODEL_SLOP)) / (UPPER_BOUND - LOWER_BOUND) as f64;
    }

    // Allocate new arenas so that we have capacity for at least slop_factor times the actually used nodes.
    let new_arenas = (active_node_count as f64 * slop_factor).ceil() as u32;
    while self.nr_arenas < new_arenas {
      self.allocate_new_arena();
    }

    // Reset state variables
    self.current_arena_past_active_arena = false;
    self.current_arena = self.first_arena;
    { // Scope of current_arena
      let current_arena = self.current_arena.as_mut_unchecked();
      self.next_node = current_arena.first_node();
      match current_arena.next_arena.is_null() {
        true => {
          // The last arena in the linked list is given a reserve.
          self.end_pointer = self.next_node.add(ARENA_SIZE - RESERVE_SIZE);
        },
        false => {
          self.end_pointer = self.next_node.add(ARENA_SIZE);
        }
      }
    }
    self.need_to_collect_garbage = false;

    #[cfg(feature = "gc_debug")]
    {
      eprintln!("end of GC");
      self.dump_memory_variables();
    }
  }

  /// Tidy up lazy sweep phase - clear marked flags and call dtors where necessary.
  unsafe fn sweep_arenas(&mut self) {
    #[cfg(feature = "gc_debug")]
    {
      eprintln!("sweep_arenas()");
      self.dump_memory_variables();
    }

    let mut new_last_active_arena = self.current_arena;
    // self.next_node never points to first node, so subtract 1.
    let mut new_last_active_node  = self.next_node.sub(1);

    if !self.current_arena_past_active_arena {
      // First tidy arenas from current up to last_active.
      let mut d = self.next_node;
      let mut c = self.current_arena;

      while c != self.last_active_arena {
        let e = c.as_mut_unchecked().first_node().add(ARENA_SIZE);

        while d != e {
          let d_mut = d.as_mut_unchecked();

          if d_mut.is_marked() {
            new_last_active_arena = c;
            new_last_active_node  = d;
            d_mut.flags.remove(DagNodeFlag::Marked);
          }
          else {
            if d_mut.needs_destruction() {
              drop_in_place(d);
            }
            d_mut.flags = DagNodeFlags::empty();
          }

          d = d.add(1);
        } // end loop over nodes

        c = c.as_mut_unchecked().next_arena;
        d = c.as_mut_unchecked().first_node();
      } // end loop over arenas

      // Now tidy last_active_arena from d upto and including last_active_node.
      let e = self.last_active_node;
      while d != e {
        let d_mut = d.as_mut_unchecked();

        if d_mut.is_marked() {
          new_last_active_arena = c;
          new_last_active_node  = d;
          d_mut.flags.remove(DagNodeFlag::Marked);
        }
        else {
          if d_mut.needs_destruction() {
            drop_in_place(d);
          }
          d_mut.flags = DagNodeFlags::empty();
        }

        d = d.add(1);
      } // end loop overactive nodes
    }

    self.last_active_arena = new_last_active_arena;
    self.last_active_node  = new_last_active_node;
  }

  /// Verify that no `DagNode` objects within the arenas managed by the allocator are in a “marked” state.
  #[cfg(feature = "gc_debug")]
  unsafe fn check_invariant(&self) {
    let mut arena     = self.first_arena;
    let mut arena_idx = 0u32;

    while !arena.is_null() {
      let arena_mut = arena.as_mut_unchecked();
      let mut d     = arena_mut.first_node();

      let bound: usize =
          match arena == self.current_arena {

            true => {
              ((self.next_node as isize - d as isize) / size_of::<DagNode>() as isize) as usize
            },

            false => ARENA_SIZE

          };

      for node_idx in 0..bound {
        if d.as_ref_unchecked().is_marked() {
          eprintln!("check_invariant() : MARKED DagNode! arena = {} node = {}", arena_idx, node_idx);
        }
        d = d.add(1);
      } // end loop over nodes

      if arena == self.current_arena { break; }

      arena = arena_mut.next_arena;
      arena_idx += 1;
    } // end loop over arenas
  }

  #[cfg(feature = "gc_debug")]
  unsafe fn check_arenas(&self) {
    let mut arena     = self.first_arena;
    let mut arena_idx = 0u32;

    while !arena.is_null() {
      let arena_mut = arena.as_mut_unchecked();
      let mut d     = arena_mut.first_node();

      for node_idx in 0..ARENA_SIZE {
        if d.as_ref_unchecked().is_marked() {
          eprintln!("check_arenas() : MARKED DagNode! arena = {} node = {}", arena_idx, node_idx);
        }
        d = d.add(1);
      } // end loop over nodes

      if arena == self.current_arena { break; }

      arena = arena_mut.next_arena;
      arena_idx += 1;
    } // end loop over arenas
  }

  /// Prints the state of the allocator.
  #[cfg(feature = "gc_debug")]
  pub fn dump_memory_variables(&self) {
    eprintln!("--------------------------------------");
    eprintln!(
      "\tnrArenas = {}\n\
            \tnrNodesInUse = {}\n\
            \tcurrentArenaPastActiveArena = {}\n\
            \tneedToCollectGarbage = {}\n\
            \tfirstArena = {:p}\n\
            \tlastArena = {:p}\n\
            \tcurrentArena = {:p}\n\
            \tnextNode = {:p}\n\
            \tendPointer = {:p}\n\
            \tlastActiveArena = {:p}\n\
            \tlastActiveNode = {:p}",
      self.nr_arenas,
      ACTIVE_NODE_COUNT.load(Relaxed),
      self.current_arena_past_active_arena,
      self.need_to_collect_garbage,
      self.first_arena,
      self.last_arena,
      self.current_arena,
      self.next_node,
      self.end_pointer,
      self.last_active_arena,
      self.last_active_node
    );
  }

}



#[cfg(test)]
mod tests {
  use crate::dag_node::DagNodeKind;
  use super::*;

  #[test]
  fn test_allocate_dag_node() {
    let mut allocator = Allocator::new();
    let node_ptr = allocator.allocate_dag_node();
    let node_mut = match unsafe { node_ptr.as_mut() } {
      None => {
        panic!("allocate_dag_node returned None");
      }
      Some(node) => { node }
    };

    node_mut.kind = DagNodeKind::Free;

  }
}

