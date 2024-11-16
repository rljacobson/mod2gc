/*!

A vector allocated from Bucket memory.

*/

use std::{
  ops::{Index, IndexMut},
  // pin::Pin,
  marker::PhantomPinned
};
use crate::dag_node::allocator::acquire_allocator;
use crate::dag_node::node::DagNodePtr;

pub type NodeVectorMutRef = &'static mut NodeVector; //Pin<&'static mut NodeVector>;
pub type NodeVectorRef    = &'static NodeVector;     //Pin<&'static NodeVector>;

pub struct NodeVector {
  length:   usize,
  capacity: usize,
  data:     &'static mut [DagNodePtr],

  // Opt out of `Unpin`
  _pin: PhantomPinned,
}

impl NodeVector {

  // region Constructors

  /// Creates a new empty vector with the given capacity.
  pub fn with_capacity(capacity: usize) -> NodeVectorMutRef {
    unsafe {
      let needed_memory = size_of::<NodeVector>() + capacity * size_of::<DagNodePtr>();
      let node_vector_ptr: *mut NodeVector = 
          acquire_allocator().allocate_storage(needed_memory) as *mut NodeVector;
      let node_vector: &mut NodeVector = node_vector_ptr.as_mut_unchecked();

      node_vector.length   = 0;
      node_vector.capacity = capacity;
      let data_ptr         = node_vector_ptr.add(size_of::<NodeVector>()) as *mut DagNodePtr;
      node_vector.data     = std::slice::from_raw_parts_mut(data_ptr, capacity);

      // Pin::new_unchecked(node_vector)
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
      // let new_vector_mut = unsafe { new_vector.as_mut().get_unchecked_mut() };
      
      new_vector.length = self.length;

      new_vector
    }
  }

  // endregion Constructors

  // Immutable iterator
  pub fn iter(&'static self) -> std::slice::Iter<'static, DagNodePtr> {
    self.data.iter()
  }

  // Mutable iterator
  pub fn iter_mut(&'static mut self) -> std::slice::IterMut<'static, DagNodePtr> {
    // Safety: Accessing `data` does not violate pinning guarantees
    // unsafe { self.get_unchecked_mut().data.iter_mut() }
    self.data.iter_mut()
  }

  pub fn len(&self) -> usize {
    self.length
  }

  pub fn capacity(&self) -> usize {
    self.capacity
  }

  pub fn is_empty(&self) -> bool { self.len() == 0 }

  /// Pushes the given node onto the (end) of the vector if there is enough capacity.
  pub fn push(&mut self, node: DagNodePtr) -> Result<(), ()> {
    if self.length >= self.capacity {
      return Err(());
    }
    // unsafe{
    //   let this = self.as_mut().get_unchecked_mut();
    //   this.data[this.length] = node;
    //   this.length += 1;
    // }
    self.data[self.length] = node;
    self.length += 1;
    Ok(())
  }

  pub fn pop(&mut self) -> Option<DagNodePtr> {
    if self.length == 0 {
      return None;
    }
    // unsafe{ self.as_mut().get_unchecked_mut().length -= 1;}
    self.length -= 1;
    
    Some(self.data[self.length])
  }
}

impl Index<usize> for NodeVector {
  type Output = DagNodePtr;

  fn index(&self, index: usize) -> &Self::Output {
    &self.data[index]
  }
}

impl IndexMut<usize> for NodeVector {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
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
