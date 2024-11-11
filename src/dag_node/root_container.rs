/*!

A `RootContainer` is a linked list of roots of garbage collected objects. 

*/

use std::sync::atomic::{
  AtomicPtr, 
  Ordering
};
use crate::dag_node::node::DagNode;

static LIST_HEAD: AtomicPtr<RootContainer> = AtomicPtr::new(std::ptr::null_mut());

pub struct RootContainer {
  next: Option<*mut RootContainer>,
  prev: Option<*mut RootContainer>,
  node: Option<*mut DagNode>
}

impl RootContainer {
  pub fn new(node: *mut DagNode) -> RootContainer {
    let maybe_node = if node.is_null() {
      None
    } else { 
      Some(node)
    };
    let mut container = RootContainer {
      next: None,
      prev: None,
      node: maybe_node
    };
    if !maybe_node.is_none() {
      container.link();
    }
    container
  }
  
  pub fn mark(&mut self) {
    unsafe {
      if let Some(node) = self.node {
        node.as_mut_unchecked().mark();
      }
    }
  }
  
  pub fn link(&mut self){
    self.prev = None;
    self.next = unsafe { 
      LIST_HEAD.load(Ordering::Relaxed)
               .as_mut()
               .map(|head| head as *mut RootContainer) 
    };
    
    if let Some(next) = self.next {
      unsafe {
        next.as_mut_unchecked().prev = Some(self);
      }
    }
    
    LIST_HEAD.store(self, Ordering::Relaxed);
  }
  
  pub fn unlink(&mut self){
    if let Some(next) = self.next {
      unsafe {
        next.as_mut_unchecked().prev = self.prev;
      }
    }
    
    if let Some(prev) = self.prev {
      unsafe {
        prev.as_mut_unchecked().next = self.next;
      }
    } else if let Some(next) = self.next {
      LIST_HEAD.store(next, Ordering::Relaxed);
    } else {
      LIST_HEAD.store(std::ptr::null_mut(), Ordering::Relaxed);
    }
  }
  
}

impl Drop for RootContainer {
  fn drop(&mut self) {
    if self.node.is_some() {
      self.unlink();
    }
  }
}

/// Marks all roots in the linked list of `RootContainer`s.
pub fn mark_roots() {
  let mut root = unsafe {
    LIST_HEAD.load(Ordering::Relaxed)
             .as_mut()
             .map(|head| head as *mut RootContainer)
  };
  unsafe {
    loop {
      match root {
        None => break,
        Some(root_ptr) => {
          let root_ref = root_ptr.as_mut_unchecked();
          root_ref.mark();
          root = root_ref.next;
        }
      }
    }
  }
}
