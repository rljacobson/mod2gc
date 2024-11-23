/*!

A vector allocated from Bucket storage.

*/

use std::{
  ops::{Index, IndexMut},
  marker::PhantomPinned
};

use std::cmp::min;

use crate::{
  dag_node::{
    allocator::storage_allocator::acquire_storage_allocator,
    node::DagNodePtr
  }
};


pub type NodeVectorMutRef = &'static mut NodeVector;
pub type NodeVectorRef    = &'static NodeVector;


pub struct NodeVector {
  length  : usize,
  capacity: usize,
  data    : &'static mut [DagNodePtr],

  // Opt out of `Unpin`
  _pin    : PhantomPinned,
}


impl NodeVector {

  // region Constructors

  /// Creates a new empty vector with the given capacity.
  pub fn with_capacity(capacity: usize) -> NodeVectorMutRef {
    unsafe {
      let node_vector_ptr: *mut NodeVector =
          { acquire_storage_allocator().allocate_storage(size_of::<NodeVector>()) as *mut NodeVector };
      let node_vector: &mut NodeVector     = node_vector_ptr.as_mut_unchecked();

      // Initialize the NodeVector
      node_vector.length   = 0;
      node_vector.capacity = capacity;

      // Allocate the memory slice. Two separate allocations are needed to maintain alignment.
      let needed_memory    = capacity * size_of::<DagNodePtr>();
      let data_ptr         = { acquire_storage_allocator().allocate_storage(needed_memory) as *mut DagNodePtr };
      node_vector.data     = std::slice::from_raw_parts_mut(data_ptr, capacity);

      node_vector
    }
  }

  /// Creates a new `NodeVector` from the given slice. The capacity of the
  /// new `NodeVector` is equal to its length.
  pub fn from_slice(vec: &[DagNodePtr]) -> NodeVectorMutRef {
    let capacity = vec.len();

    let node_vector_mut: NodeVectorMutRef = NodeVector::with_capacity(capacity);
    // let node_vector_mut: &mut NodeVector = unsafe{ node_vector.as_mut().get_unchecked_mut() };

    // Copy contents of vec into node_vector.data
    for (i, &item) in vec.iter().enumerate() {
      node_vector_mut.data[i] = item;
    }

    node_vector_mut.length = capacity;

    node_vector_mut
  }

  /// Creates an identical shallow copy, allocating new memory. The copy
  /// has the same capacity as the original.
  pub fn shallow_copy(&self) -> NodeVectorMutRef {
    NodeVector::copy_with_capacity(self, self.capacity)
  }

  /// Makes a copy of this node but with `new_capacity`. If `self.length` > `new_capacity`,
  /// nodes are truncated.
  pub fn copy_with_capacity(&self, new_capacity: usize) -> NodeVectorMutRef {
    // let this: &NodeVector = self.as_ref().get_ref();

    if new_capacity > self.capacity {
      let new_vector_mut: NodeVectorMutRef = NodeVector::with_capacity(new_capacity);
      // let new_vector_mut: &mut NodeVector  = unsafe { new_vector.as_mut().get_unchecked_mut() };

      new_vector_mut.length = self.length;

      for i in 0..self.length {
        new_vector_mut.data[i] = self.data[i];
      }

      new_vector_mut
    }
    else {
      // To keep things simple, we copy everything up to `new_capacity` even if
      // `length` is shorter.
      let new_vector = NodeVector::from_slice(&self.data[0..new_capacity]);

      new_vector.length = min(self.length, new_capacity);

      new_vector
    }
  }

  // endregion Constructors

  // Immutable iterator
  pub fn iter(&'static self) -> std::slice::Iter<'static, DagNodePtr> {
    self.data[..self.length].iter()
  }

  // Mutable iterator
  pub fn iter_mut(&'static mut self) -> std::slice::IterMut<'static, DagNodePtr> {
    self.data[..self.length].iter_mut()
  }

  pub fn len(&self) -> usize {
    self.length
  }

  pub fn capacity(&self) -> usize {
    self.capacity
  }

  pub fn is_empty(&self) -> bool { self.len() == 0 }

  /// Pushes the given node onto the (end) of the vector if there is enough capacity.
  pub fn push(&mut self, node: DagNodePtr) -> Result<(), String> {
    if self.length >= self.capacity 
        || self.data.len() > self.capacity
        || self.data.len() <= self.length
    {
      panic!("node_vec.len: {}, capacity: {}, data.len: {}", self.length, self.capacity, self.data.len());
      // acquire_allocator().dump_memory_variables(); // Can cause deadlock

      return Err(format!("node_vec.len: {}, capacity: {}, data.len: {}", self.length, self.capacity, self.data.len()));
    }

    self.data[self.length] = node;
    self.length += 1;
    Ok(())
  }

  pub fn pop(&mut self) -> Option<DagNodePtr> {
    if self.length == 0 {
      return None;
    }

    self.length -= 1;

    Some(self.data[self.length])
  }
}

impl Index<usize> for NodeVector {
  type Output = DagNodePtr;

  fn index(&self, index: usize) -> &Self::Output {
    assert!(index < self.length);
    &self.data[index]
  }
}

impl IndexMut<usize> for NodeVector {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    assert!(index < self.length);
    &mut self.data[index]
  }
}

impl<'a> IntoIterator for &'a NodeVector {
  type Item = &'a DagNodePtr;
  type IntoIter = std::slice::Iter<'a, DagNodePtr>;

  fn into_iter(self) -> Self::IntoIter {
    self.data.iter()
  }
}

impl<'a> IntoIterator for &'a mut NodeVector {
  type Item = &'a mut DagNodePtr;
  type IntoIter = std::slice::IterMut<'a, DagNodePtr>;

  fn into_iter(self) -> Self::IntoIter {
    self.data.iter_mut()
  }
}
