/*!

A `RootContainer` is a linked list of roots of garbage collected objects.

*/

use std::sync::atomic::{
  AtomicPtr,
  Ordering
};
use std::sync::Mutex;
use crate::dag_node::node::DagNode;

static LIST_HEAD: Mutex<AtomicPtr<RootContainer>> = Mutex::new(AtomicPtr::new(std::ptr::null_mut()));

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
    let list_head = LIST_HEAD.lock().unwrap();
    self.prev = None;
    self.next = unsafe {
      list_head.load(Ordering::Relaxed)
               .as_mut()
               .map(|head| head as *mut RootContainer)
    };

    if let Some(next) = self.next {
      unsafe {
        next.as_mut_unchecked().prev = Some(self);
      }
    }

    list_head.store(self, Ordering::Relaxed);
  }

  pub fn unlink(&mut self){
    let list_head = LIST_HEAD.lock().unwrap();
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
      list_head.store(next, Ordering::Relaxed);
    } else {
      list_head.store(std::ptr::null_mut(), Ordering::Relaxed);
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
  let list_head = LIST_HEAD.lock().unwrap();
  let mut root = unsafe {
    list_head.load(Ordering::Relaxed)
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
