/*!

An arena allocator for `DagNode`s. 

*/

use std::alloc::{alloc_zeroed, Layout};
use crate::dag_node::ARENA_SIZE;
use crate::dag_node::node::DagNode;

#[repr(align(8))]
pub struct Arena {
  pub(crate) next_arena: *mut Arena,
  data: [DagNode; ARENA_SIZE],
}

impl Arena {
  #[inline(always)]
  pub fn allocate_new_arena() -> *mut Arena {
    unsafe {
      let chunk: *mut u8 = alloc_zeroed(
        Layout::from_size_align(size_of::<Arena>(), 8).unwrap()
      );
      chunk as *mut Arena
    }
  }

  #[inline(always)]
  pub fn first_node(&mut self) -> *mut DagNode {
    self.data.as_mut_ptr()
  }
}