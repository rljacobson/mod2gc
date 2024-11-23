/*!

The `DagNode` is the heart of the engine. Speed hinges on efficient management of `DagNode` objects. Their creation,
reuse, and destruction are managed by an arena based garbage collecting allocator which relies on the fact that
every `DagNode` is of the same size. Since `DagNode`s can be of different types and have arguments, we make careful use
of transmute and bitflags.

The following compares Maude's `DagNode` to our implementation here.

|                | Maude                                        | mod2lib                     |
|:---------------|:---------------------------------------------|:----------------------------|
| size           | Fixed 3 word size (or 6 words?)              | Fixed size struct (3 words) |
| tag            | implicit via vtable pointer                  | enum variant                |
| flags          | `MemoryInfo` in first word                   | `BitFlags` field            |
| shared impl    | base class impl                              | enum impl                   |
| specialization | virtual function calls                       | match on variant in impl    |
| args           | `reinterpret_cast` of 2nd word based on flag | Nested enum                 |

*/


mod flags;
mod node;
mod root_container;
pub mod allocator;

pub use node::*;
pub use flags::*;
pub use root_container::RootContainer;

/// A `*mut Void` is a pointer to a `u8`
pub type Void = u8;

// To determine bucket usage target


#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum DagNodeKind {
  #[default]
  Free = 0,
  ACU,
  AU,
  CUI,
  Variable,
  NA,
  Data,
  // Integer,
  // Float
}

#[cfg(test)]
mod tests {
  use crate::dag_node::DagNodeKind;
  use crate::dag_node::flags::DagNodeFlags;
  use crate::dag_node::node::{DagNode, DagNodeArgument};
  use crate::symbol::SymbolPtr;

  #[test]
  fn size_of_dag_node() {
    println!("size of SymbolPtr: {}", size_of::<SymbolPtr>());
    println!("size of DagNodeArgument: {}", size_of::<DagNodeArgument>());
    println!("size of DagNodeKind: {}", size_of::<DagNodeKind>());
    println!("size of DagNodeFlags: {}", size_of::<DagNodeFlags>());
    println!("size of DagNode: {}", size_of::<DagNode>());
    assert_eq!(size_of::<DagNode>(), 4 * size_of::<usize>());
  }
}
