/*!

A `RootContainer` is a linked list of roots of garbage collected objects.

*/

use std::{
  sync::{
    atomic::{
      AtomicPtr,
      Ordering
    },
    Mutex
  }
};
use std::ptr::NonNull;
use std::sync::MutexGuard;
use crate::dag_node::node::DagNode;

static LIST_HEAD: Mutex<AtomicPtr<RootContainer>> = Mutex::new(AtomicPtr::new(std::ptr::null_mut()));

pub fn acquire_root_list() -> MutexGuard<'static, AtomicPtr<RootContainer>> {
  LIST_HEAD.lock().unwrap()
}

pub struct RootContainer {
  next: Option<NonNull<RootContainer>>,
  prev: Option<NonNull<RootContainer>>,
  node: Option<NonNull<DagNode>>
}

impl RootContainer {
  pub fn new(node: *mut DagNode) -> RootContainer {
    let maybe_node = NonNull::new(node);
    let mut container = RootContainer {
      next: None,
      prev: None,
      node: maybe_node
    };
    // We only add the container to the linked list if it holds a node.
    if !maybe_node.is_none() {
      container.link();
    }
    container
  }

  pub fn mark(&mut self) {
    unsafe {
      if let Some(mut node) = self.node {
        node.as_mut().mark();
      }
    }
  }

  pub fn link(&mut self){
    let list_head  = acquire_root_list();
    self.prev = None;
    self.next = unsafe {
      list_head.load(Ordering::Relaxed)
               .as_mut()
               .map(|head| NonNull::new(head as *mut RootContainer).unwrap())
    };

    if let Some(mut next) = self.next {
      unsafe {
        next.as_mut().prev = NonNull::new(self);
      }
    }

    list_head.store(self, Ordering::Relaxed);
  }

  pub fn unlink(&mut self){
    let list_head = acquire_root_list();
    if let Some(mut next) = self.next {
      unsafe {
        next.as_mut().prev = self.prev;
      }
    }

    if let Some(mut prev) = self.prev {
      unsafe {
        prev.as_mut().next = self.next;
      }
    } else if let Some(mut next) = self.next {
      list_head.store(next.as_ptr(), Ordering::Relaxed);
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
  let list_head = acquire_root_list();
  let mut root = unsafe {
    list_head.load(Ordering::Relaxed)
             .as_mut()
             .map(|head| NonNull::new(head as *mut RootContainer).unwrap())
  };

  loop {
    match root {
      None => break,
      Some(mut root_ptr) => {
        assert!(!root_ptr.as_ptr().is_null());
        let root_ref = unsafe{ root_ptr.as_mut() };
        root_ref.mark();
        root = root_ref.next;
      }
    }
  }
}
