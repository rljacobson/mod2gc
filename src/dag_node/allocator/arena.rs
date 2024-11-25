/*!

An arena allocator for `DagNode`s.

*/

use std::{
  mem::MaybeUninit,
  ptr::null_mut
};

use crate::{
  dag_node::{
    allocator::node_allocator::ARENA_SIZE,
    node::DagNode
  }
};

#[repr(align(8))]
pub struct Arena {
  pub(crate) next_arena: *mut Arena,
  data: [DagNode; ARENA_SIZE],
}

impl Arena {
  #[inline(always)]
  pub fn allocate_new_arena() -> *mut Arena {

    // Create an uninitialized array
    let data: [MaybeUninit<DagNode>; ARENA_SIZE] = unsafe { MaybeUninit::uninit().assume_init() };
    
    /* Each node is initialized on allocation, so we don't bother with this.
    // Initialize each element
    for elem in &mut data {
      unsafe {
        std::ptr::write(elem.as_mut_ptr(), DagNode::default());
      }
    }
    */
    
    let arena = Box::new(Arena{
      next_arena: null_mut(),
      data      : unsafe { std::mem::transmute::<_, [DagNode; ARENA_SIZE]>(data) }
    });

    Box::into_raw(arena)
  }

  #[inline(always)]
  pub fn first_node(&mut self) -> *mut DagNode {
    &mut self.data[0]
  }
}
