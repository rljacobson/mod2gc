/*!

# Arena Allocator
See GarbageCollector.md for a detailed explanation of how it works. Below is a brief summary of how it works.

The arena allocator manages memory by organizing it into arenas, which are fixed size arrays of nodes available for allocation. The allocator uses a simple mark-and-sweep algorithm to collect garbage, but the sweep phase is "lazy." When the program requests a new node allocation, the allocator searches linearly for free nodes within these arenas and reuses them when possible. During this linear search, the allocator performs a "lazy sweep," clearing all "marked" flags on nodes and running destructors when necessary. This proceeds until either an available node is found and returned or all nodes are found to be in use, in which case it may expand by creating a new arena or adding capacity to existing ones.

When garbage collection is triggered, the allocator then sweeps the remaining (not yet searched) part of the arena(s). Then it begins the mark phase. During marking, the allocator requests all node roots to flag nodes that are actively in use so that they’re preserved. During this phase, the number of active nodes is computed. After marking, the allocator compares it's total node capacity to the number of active nodes and, if the available capacity is less than a certain "slop factor," more arenas are allocated from system memory. The "cursor" for the linear search is then reset to the first node of the first arena.

Since the sweep phase is done lazily, the time it takes to sweep the arenas is amortized between garbage collection events. Because garbage collection is triggered when the linear search for free nodes nears the end of the last arena, allocating a "slop factor" of extra arenas keeps garbage collection events low.

*/

use std::{
  sync::{
    atomic::{
      Ordering::Relaxed,
      AtomicUsize
    },
    Mutex,
    MutexGuard,
  },
  ptr::drop_in_place,
};

use once_cell::sync::Lazy;

use crate::{
  dag_node::{
    allocator::arena::Arena,
    DagNode,
    DagNodeFlag,
    DagNodeFlags,
    root_container::mark_roots
  }
};
use crate::dag_node::allocator::storage_allocator::acquire_storage_allocator;

// Constant Allocator Parameters
pub const SMALL_MODEL_SLOP: f64   = 8.0;
pub const BIG_MODEL_SLOP  : f64   = 2.0;
pub const LOWER_BOUND     : usize =  4 * 1024 * 1024; // Use small model if <= 4 million nodes
pub const UPPER_BOUND     : usize = 32 * 1024 * 1024; // Use big model if >= 32 million nodes
// It looks like Maude assumes DagNodes are 6 words in size, but ours are 3 words,
// at least so far.
pub(crate) const ARENA_SIZE: usize = 5460; // Arena size in nodes; 5460 * 6 + 1 + new/malloc_overhead <= 32768 words
pub const RESERVE_SIZE         : usize = 256; // If fewer nodes left call GC when allowed


pub static ACTIVE_NODE_COUNT: AtomicUsize = AtomicUsize::new(0);
static GLOBAL_NODE_ALLOCATOR: Lazy<Mutex<NodeAllocator>> = Lazy::new(|| {
  Mutex::new(NodeAllocator::new())
});


#[inline(always)]
pub fn acquire_node_allocator(caller_msg: &str) -> MutexGuard<'static, NodeAllocator> {
  match GLOBAL_NODE_ALLOCATOR.try_lock() {
    Ok(allocator) => allocator,
    Err(e) => {
      panic!("Global node allocator is deadlocked: {}", caller_msg);
    }
  }
}


pub struct NodeAllocator {
  // General settings
  show_gc   : bool, // Do we report GC stats to user
  early_quit: u64,  // Do we quit early for profiling purposes

  need_to_collect_garbage        : bool,

  // Arena management variables
  arena_count: u32,
  current_arena_past_active_arena: bool,
  first_arena                    : *mut Arena,
  last_arena                     : *mut Arena,
  current_arena                  : *mut Arena,
  next_node                      : *mut DagNode,
  end_pointer                    : *mut DagNode,
  last_active_arena              : *mut Arena,
  last_active_node               : *mut DagNode,
}

// Access is hidden behind a mutex.
unsafe impl Send for NodeAllocator {}
// unsafe impl Sync for Allocator {}

impl NodeAllocator {
  pub fn new() -> Self {
    NodeAllocator {
      show_gc    : true,
      early_quit : 0,
      arena_count: 0,

      current_arena_past_active_arena: false,
      need_to_collect_garbage        : false,

      first_arena      : std::ptr::null_mut(),
      last_arena       : std::ptr::null_mut(),
      current_arena    : std::ptr::null_mut(),
      next_node        : std::ptr::null_mut(),
      end_pointer      : std::ptr::null_mut(),
      last_active_arena: std::ptr::null_mut(),
      last_active_node : std::ptr::null_mut(),
    }
  }

  /// Tell the garbage collect to collect garbage if it needs to.
  /// You can query whether it needs to by calling `want_to_collect_garbage`,
  /// but this isn't necessary.
  #[inline(always)]
  pub fn ok_to_collect_garbage(&mut self) {
    if self.need_to_collect_garbage
        || acquire_storage_allocator().want_to_collect_garbage()
    {
      unsafe{ self.collect_garbage(); }
    }
  }

  /// Query whether the allocator has any garbage to collect.
  #[inline(always)]
  pub fn want_to_collect_garbage(&self) -> bool {
    self.need_to_collect_garbage
  }

  /// Allocates a new `DagNode`
  pub fn allocate_dag_node(&mut self) -> *mut DagNode {
    // ToDo: I think we can replace these pointers with indices into the current arena's data array.
    //       Includes next_node, end_pointer, end_node.
    let mut current_node = self.next_node;

    unsafe{
      loop {
        if (current_node.is_null() && self.end_pointer.is_null()) || current_node == self.end_pointer {
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
    self.arena_count += 1;

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
        
        self.last_active_arena = arena;
        self.last_active_node  = first_node;

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
      while cursor != end_node {
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

  unsafe fn collect_garbage(&mut self) {
    static mut GC_COUNT: u64 = 0;

    if self.first_arena.is_null() {
      return;
    }

    self.sweep_arenas();
    #[cfg(feature = "gc_debug")]
    self.check_arenas();

    // Mark phase

    let old_active_node_count = active_node_count();
    ACTIVE_NODE_COUNT.store(0, Relaxed); // to be updated during mark phase.

    acquire_storage_allocator()._prepare_to_mark();

    mark_roots();

    acquire_storage_allocator()._sweep_garbage();

    // Garbage Collection for Arenas

    let active_node_count = active_node_count(); // updated during mark phase

    // Calculate if we should allocate more arenas to avoid an early gc.
    let node_capacity = (self.arena_count as usize) * ARENA_SIZE;
    GC_COUNT += 1;
    let gc_count = GC_COUNT; // To silence shared_mut_ref warning

    if self.show_gc {
      println!("Collection: {}", gc_count);

      println!(
        "Arenas: {}\tNodes: {} ({:.2} MB)\tCollected: {} ({:.2}) MB\tNow: {} ({:.2} MB)",
        self.arena_count,
        node_capacity,
        ((node_capacity * size_of::<DagNode>()) as f64) / (1024.0 * 1024.0),
        old_active_node_count - active_node_count,
        (((old_active_node_count - active_node_count) * size_of::<DagNode>() ) as f64) / (1024.0 * 1024.0),
        active_node_count,
        ((active_node_count * size_of::<DagNode>()) as f64) / (1024.0 * 1024.0),
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
      slop_factor += ((UPPER_BOUND - active_node_count as usize) as f64 * (SMALL_MODEL_SLOP - BIG_MODEL_SLOP)) / (UPPER_BOUND - LOWER_BOUND) as f64;
    }

    // Allocate new arenas so that we have capacity for at least slop_factor times the actually used nodes.
    let new_arenas = (active_node_count as f64 * slop_factor / (ARENA_SIZE as f64)).ceil() as u32;
    // let new_arenas = 2;
    #[cfg(feature = "gc_debug")]
    println!("new_arenas: {}", new_arenas);
    while self.arena_count < new_arenas {
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
      let mut node_cursor_ptr: *mut DagNode = self.next_node;
      let mut arena_cursor: *mut Arena = self.current_arena;

      while arena_cursor != self.last_active_arena {
        let end_node_ptr = arena_cursor.as_mut_unchecked().first_node().add(ARENA_SIZE);

        while node_cursor_ptr != end_node_ptr {
          let node_cursor_mut = node_cursor_ptr.as_mut_unchecked();

          if node_cursor_mut.is_marked() {
            new_last_active_arena = arena_cursor;
            new_last_active_node  = node_cursor_ptr;
            node_cursor_mut.flags.remove(DagNodeFlag::Marked);
          }
          else {
            if node_cursor_mut.needs_destruction() {
              drop_in_place(node_cursor_ptr);
            }
            node_cursor_mut.flags = DagNodeFlags::empty();
          }

          node_cursor_ptr = node_cursor_ptr.add(1);
        } // end loop over nodes

        arena_cursor = arena_cursor.as_mut_unchecked().next_arena;
        node_cursor_ptr = arena_cursor.as_mut_unchecked().first_node();
        
      } // end loop over arenas

      // Now tidy last_active_arena from d upto and including last_active_node.
      let end_node_ptr = self.last_active_node;
      let check_the_start = self.last_active_arena.as_mut_unchecked().first_node();
      let check_the_end = check_the_start.add(ARENA_SIZE);
      if check_the_start > node_cursor_ptr || check_the_end < node_cursor_ptr
      {
        panic!("node_cursor_ptr is not in last active arena");
      }
      
      let mut counter: u32 = 0;
      while node_cursor_ptr <= end_node_ptr {
        counter += 1;
        let d_mut = node_cursor_ptr.as_mut_unchecked();

        if d_mut.is_marked() {
          new_last_active_arena = arena_cursor;
          new_last_active_node  = node_cursor_ptr;
          d_mut.flags.remove(DagNodeFlag::Marked);
        }
        else {
          if d_mut.needs_destruction() {
            drop_in_place(node_cursor_ptr);
          }
          d_mut.flags = DagNodeFlags::empty();
        }

        node_cursor_ptr = node_cursor_ptr.add(1);
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
            \tactive_node_count = {}\n\
            \tneed_to_collect_garbage = {}\n\
            \tcurrent_arena_past_active_arena = {}\n\
            \tneed_to_collect_garbage = {}\n\
            \tfirst_arena = {:p}\n\
            \tlast_arena = {:p}\n\
            \tcurrent_arena = {:p}\n\
            \tnext_node = {:p}\n\
            \tend_pointer = {:p}\n\
            \tlast_active_arena = {:p}\n\
            \tlast_active_node = {:p}",
      self.arena_count,
      ACTIVE_NODE_COUNT.load(Relaxed),
      self.need_to_collect_garbage,
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




#[inline(always)]
pub fn increment_active_node_count() {
  ACTIVE_NODE_COUNT.fetch_add(1, Relaxed);
}

#[inline(always)]
pub fn active_node_count() -> usize {
  ACTIVE_NODE_COUNT.load(Relaxed)
}


#[cfg(test)]
mod tests {
  use crate::abstractions::IString;
  use crate::dag_node::{DagNode, DagNodeKind, DagNodePtr, RootContainer};
  use crate::dag_node::allocator::*;
  use crate::dag_node::allocator::NodeAllocator;
  use crate::symbol::Symbol;

  #[test]
  fn test_allocate_dag_node() {
    let mut allocator = NodeAllocator::new();
    let node_ptr = allocator.allocate_dag_node();
    let node_mut = match unsafe { node_ptr.as_mut() } {
      None => {
        panic!("allocate_dag_node returned None");
      }
      Some(node) => { node }
    };

    node_mut.kind = DagNodeKind::Free;
  }

  #[test]
  fn test_arena_exhaustion() {
    let mut allocator = NodeAllocator::new();
    let mut symbol = Symbol::new(IString::from("mysymbol"), 1);
    let symbol_ptr = &mut symbol;
    let root: DagNodePtr = DagNode::new(symbol_ptr);
    println!("root: {:p}", root);

    let root_container = RootContainer::new(root);
    
    let mut last_node = root;
    
    for j in 1..=10000 {
      let node_ptr = allocator.allocate_dag_node();
      let node_mut = match unsafe { node_ptr.as_mut() } {
        None => {
          panic!("allocate_dag_node returned None");
        }
        Some(node) => { 
          node 
        }
      };
      unsafe {
        (&mut*last_node).insert_child(node_ptr).expect("Could not insert child");
      }
      last_node = node_ptr;
      node_mut.kind = DagNodeKind::Free;
      // The tree is 10000 nodes deep. The mark phase will blow the stack.
      // if j % 100 == 0 {
      //   allocator.ok_to_collect_garbage();
      // }
    }
    
    allocator.dump_memory_variables();
  }
}
