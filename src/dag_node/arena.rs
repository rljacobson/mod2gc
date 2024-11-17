/*!

An arena allocator for `DagNode`s.

*/

use std::alloc::{alloc_zeroed, Layout};
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use crate::dag_node::allocator::ARENA_SIZE;
use crate::dag_node::node::DagNode;

#[repr(align(8))]
pub struct Arena {
  pub(crate) next_arena: *mut Arena,
  data: [DagNode; ARENA_SIZE],
}

impl Arena {
  #[inline(always)]
  pub fn allocate_new_arena() -> *mut Arena {

    // Create an uninitialized array
    let mut data: [MaybeUninit<DagNode>; ARENA_SIZE] = unsafe { MaybeUninit::uninit().assume_init() };

    // Initialize each element
    for elem in &mut data {
      unsafe {
        std::ptr::write(elem.as_mut_ptr(), DagNode::default()); // Replace `DagNode::new()` with your constructor
      }
    }
    // Convert the array to an initialized array
    let data = unsafe { std::mem::transmute::<_, [DagNode; ARENA_SIZE]>(data) };
    let arena = Box::new(Arena{
      next_arena: null_mut(),
      data
    });
    
    Box::into_raw(arena)
  }

  #[inline(always)]
  pub fn first_node(&mut self) -> *mut DagNode {
    self.data.as_mut_ptr()
  }
}
