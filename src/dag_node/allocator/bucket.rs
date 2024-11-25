/*!

A `Bucket` is a small arena. We might use bumpalo or something instead.

*/

use std::ptr::{null_mut, NonNull};
use crate::dag_node::Void;

pub struct Bucket {
  pub(crate) data: Box<[Void]>,
  pub(crate) bytes_free : usize,
  pub(crate) next_free  : *mut Void,
  pub(crate) next_bucket: Option<NonNull<Bucket>>,
}

impl Bucket {
  pub fn with_capacity(capacity: usize) -> Self {
    let mut bucket = Bucket {
      data       : vec![0; capacity].into_boxed_slice(),
      bytes_free : capacity,
      next_free  : null_mut(),
      next_bucket: None,
    };
    bucket.next_free = bucket.data.as_mut_ptr();
    
    bucket
  }

  pub fn allocate(&mut self, bytes_needed: usize) -> *mut Void {
    assert!(self.bytes_free >= bytes_needed);

    let allocation    = self.next_free;
    let new_next_free = unsafe { self.next_free.add(bytes_needed) };
    let align_offset  = new_next_free.align_offset(8);
    if align_offset == usize::MAX {
      panic!("Cannot align memory to 8 byte boundary")
    }
    
    // next_free is always aligned on an 8 byte boundary.
    self.next_free = unsafe { new_next_free.add(align_offset) };
    let bytes_used = bytes_needed + align_offset;
    if bytes_used > self.bytes_free {
      // This probably should happen due to how capacity for new buckets is 
      // computed, but it's conceivable.
      self.bytes_free = 0;
    } else {
      self.bytes_free -= bytes_used;
    }
    
    allocation
  }
  
  pub fn reset(&mut self) {
    self.next_free  = self.data.as_mut_ptr();
    self.bytes_free = self.data.len()
  }
}
