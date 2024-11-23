/*!

`DagNode` is the building block for the Directed Acyclic Graph and is what makes the engine fast. `DagNode`s are small
(24 bytes == 3 machine words) and garbage collected.

*/

use std::{
  cmp::max,
  fmt::{Display, Formatter},
  marker::PhantomPinned
};
use std::ptr::null_mut;
use crate::{
  dag_node::{
    flags::{
      DagNodeFlag,
      DagNodeFlags
    },
    DagNodeKind
  },
  symbol::{
    Symbol,
    SymbolPtr
  },
};
use crate::dag_node::allocator::{acquire_node_allocator, increment_active_node_count};
use crate::dag_node::allocator::node_vector::{
  NodeVector,
  NodeVectorMutRef
};

/// Public interface uses `Pin`. We need to be able to have multiple references,
/// and we occasionally need to mutate the node, so we use `*mut DagNode`
/// instead of `&mut DagNode` or `&DagNode`.
// ToDo: Should this be `NonNull<*mut DagNode>`?
pub type DagNodePtr = *mut DagNode;

#[derive(Default)]
pub enum DagNodeArgument{
  #[default]
  None,
  Single(DagNodePtr),
  Many(NodeVectorMutRef)
}

pub struct DagNode {
  pub(crate) symbol: SymbolPtr,
  args:      DagNodeArgument,
  pub kind:  DagNodeKind,
  pub flags: DagNodeFlags,

  // Opt out of `Unpin`
  _pin: PhantomPinned,
}

impl DagNode {
  // region Constructors

  pub fn new(symbol: SymbolPtr) -> DagNodePtr {
    DagNode::with_kind(symbol, DagNodeKind::default())
  }

  pub fn with_kind(symbol: SymbolPtr, kind: DagNodeKind) -> DagNodePtr {
    let node: DagNodePtr = { acquire_node_allocator("DagNode::with_kind").allocate_dag_node() };
    let node_mut         = unsafe { &mut *node };

    let arity = unsafe{ &*symbol }.arity as usize;

    node_mut.kind   = kind;
    node_mut.flags  = DagNodeFlags::empty();
    node_mut.symbol = symbol;
    node_mut.args   = if arity > 1 {
      DagNodeArgument::Many(NodeVector::with_capacity(arity))
    } else {
      DagNodeArgument::None
    };
    node
  }

  pub fn with_args(symbol: SymbolPtr, args: &mut Vec<DagNodePtr>, kind: DagNodeKind) -> DagNodePtr {
    assert!(!symbol.is_null());
    let node: DagNodePtr = { acquire_node_allocator("DagNode::with_args").allocate_dag_node() };
    let node_mut         = unsafe { &mut *node };

    node_mut.kind   = kind;
    node_mut.flags  = DagNodeFlags::empty();
    node_mut.symbol = symbol;

    let arity = unsafe{ &*symbol }.arity as usize;

    if arity > 1 || args.len() > 1 {
      let capacity = max(arity, args.len());
      let node_vector = NodeVector::with_capacity(capacity);

      for node in args.iter().cloned() {
        _  = node_vector.push(node);
      }

      node_mut.args = DagNodeArgument::Many(node_vector);
    }
    else if args.len() == 1 {
      node_mut.args = DagNodeArgument::Single(args[0]);
    } else {
      node_mut.args = DagNodeArgument::None;
    };

    node
  }

  // endregion Constructors

  // region Accessors

  pub fn iter_children(&self) -> std::slice::Iter<'static, DagNodePtr> {
    let arity = self.arity();
    match &self.args {
      DagNodeArgument::None => {
        assert_eq!(arity, 0);
        [].iter()
      }
      DagNodeArgument::Single(node) => {
        assert_eq!(arity, 1);
        // Make a fat pointer to the single node and return an iterator to it. This allows `self` to
        // escape the method. Of course, `self` actually points to a `DagNode` that is valid for the
        // lifetime of the program, so even in the event of the GC equivalent of a dangling pointer
        // or use after free, this will be safe. (Strictly speaking, it's probably UB.)
        let v = unsafe { std::slice::from_raw_parts(node, 1) };
        v.iter()
      }
      DagNodeArgument::Many(node_vector) => {
        assert!(arity>1);
        // We need to allow `self` to escape the method, same as `Single(..)` branch.
        let node_vector_ptr: *const NodeVector = *node_vector;
        unsafe{ &*node_vector_ptr }.iter()
      }
    }
  }

  #[inline(always)]
  pub fn symbol(&self) -> &Symbol {
    unsafe {
      &*self.symbol
    }
  }

  #[inline(always)]
  pub fn arity(&self) -> u8 {
    self.symbol().arity
  }

  #[inline(always)]
  pub fn len(&self) -> usize {
    match &self.args {
      DagNodeArgument::None      => 0,
      DagNodeArgument::Single(_) => 1,
      DagNodeArgument::Many(v)   => v.len()
    }
  }

  pub fn insert_child(&mut self, new_child: DagNodePtr) -> Result<(), String>{
    match self.args {

      DagNodeArgument::None => {
        self.args = DagNodeArgument::Single(new_child);
        Ok(())
      }

      DagNodeArgument::Single(first_child) => {
        let vec   = NodeVector::from_slice(&[first_child, new_child]);
        self.args = DagNodeArgument::Many(vec);
        Ok(())
      }

      DagNodeArgument::Many(ref mut vec) => {
        vec.push(new_child)
      }

    }
  }

  // endregion

  // region GC related methods
  #[inline(always)]
  pub fn is_marked(&self) -> bool {
    self.flags.contains(DagNodeFlag::Marked)
  }

  #[inline(always)]
  pub fn needs_destruction(&self) -> bool {
    // self.flags.contains(DagNodeFlag::NeedsDestruction)
    match self.args {
      DagNodeArgument::None
      | DagNodeArgument::Single(_) => false,
      DagNodeArgument::Many(_) => true,
    }
  }

  #[inline(always)]
  pub fn simple_reuse(&self) -> bool {
    !self.flags.contains(DagNodeFlag::Marked) && !self.needs_destruction()
  }

  #[inline(always)]
  pub fn mark(&'static mut self) {
    if self.flags.contains(DagNodeFlag::Marked) {
      return;
    }

    increment_active_node_count();
    self.flags.insert(DagNodeFlag::Marked);

    // Temporarily replace `self.args` with a placeholder
    let current_args = std::mem::replace(&mut self.args, DagNodeArgument::None);
    match current_args {

      DagNodeArgument::None => { /* pass */ }

      DagNodeArgument::Single(node) => {
        if let Some(node) = unsafe { node.as_mut() } {
          node.mark();
        }
        self.args = DagNodeArgument::Single(node);
      }

      DagNodeArgument::Many(node_vec) => {
        for node in node_vec.iter() {
          if let Some(node) = unsafe { node.as_mut() } {
            node.mark();
          } else {
            eprintln!("Bad node found.")
          }
        }
        // Reallocate
        let node_vec = node_vec.shallow_copy();
        self.args = DagNodeArgument::Many(node_vec);
      }

    }

  }
  //endregion

}

impl Display for DagNode {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "node<{}>", self.symbol())
  }
}

impl Default for DagNode {
  fn default() -> Self {
    DagNode{
      symbol: null_mut(),
      args: DagNodeArgument::None,
      kind: Default::default(),
      flags: Default::default(),
      _pin: Default::default(),
    }
  }
}
