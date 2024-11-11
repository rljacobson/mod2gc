/*!

`DagNode` is the building block for the Directed Acyclic Graph and is what makes the engine fast. `DagNode`s are small
(24 bytes == 3 machine words) and garbage collected.

*/

use std::{
  marker::PhantomPinned,
  pin::Pin
};
use std::ptr::drop_in_place;
use crate::{
  dag_node::{
    DagNodeKind,
    flags::{DagNodeFlag, DagNodeFlags}
  },
  symbol::{Symbol, SymbolPtr}
};

// Public interface uses `Pin`.
pub type DagNodePtr    = Pin<*mut DagNode>;


union DagNodeArgument{
  single: *mut DagNode, 
  many: *mut *mut DagNode
}

pub struct DagNode {
  pub(crate) symbol: SymbolPtr,
  args:      DagNodeArgument,
  pub kind:  DagNodeKind,
  pub flags: DagNodeFlags,

  // Opt out of `Unpin`
  _pin: PhantomPinned,
}

impl Default for DagNode {
  fn default() -> Self {
    DagNode {
      symbol: std::ptr::null_mut(),
      args  : DagNodeArgument {
        single: std::ptr::null_mut()
      },
      kind : DagNodeKind::default(),
      flags: DagNodeFlags::default(),
      _pin : PhantomPinned::default()
    }
  }
}

impl DagNode {

  // region Accessors
  #[inline(always)]
  pub fn symbol(&self) -> Pin<&Symbol> {
    unsafe {
      Pin::new(&*self.symbol)
    }
  }
  
  #[inline(always)]
  pub fn arity(&self) -> u8 {
    self.symbol().arity
  }
  // endregion

  // region GC related methods
  #[inline(always)]
  pub fn is_marked(&self) -> bool {
    self.flags.contains(DagNodeFlag::Marked)
  }

  #[inline(always)]
  pub fn needs_destruction(&self) -> bool {
    self.flags.contains(DagNodeFlag::NeedsDestruction)
  }

  #[inline(always)]
  pub fn simple_reuse(&self) -> bool {
    !self.flags.contains(DagNodeFlag::Marked) && !self.flags.contains(DagNodeFlag::NeedsDestruction)
  }
  
  #[inline(always)]
  pub fn mark(&mut self) {
    self.flags.insert(DagNodeFlag::Marked);
  }
  //endregion
  
}

impl Drop for DagNode {
  fn drop(&mut self) {
    inner_drop(unsafe { Pin::new_unchecked(self)});

    fn inner_drop(this: Pin<&mut DagNode>) {
      if this.needs_destruction(){
        // `this.args` is an array of `*mut DagNode`.
        unsafe {
          let args_ptr = this.args.many;
          let arity = this.arity();
          for i in 0..arity as usize {
            // Get a pointer to the i-th `*mut DagNode`
            let arg_ptr = args_ptr.add(i);
            // Drop the `DagNode` at `*arg_ptr`
            drop_in_place(*arg_ptr);
          }
        }
      }
      else {
        // `this.args` is a single `*mut DagNode`.
        unsafe {
          if !this.args.single.is_null() {
            drop_in_place(this.args.single);
          }
        }
      }
    } // end inner_drop

  } // end drop
}
