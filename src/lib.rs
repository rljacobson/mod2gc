#![feature(ptr_as_ref_unchecked)]
#![allow(dead_code)]
extern crate core;

mod symbol;
mod abstractions;
mod dag_node;
mod util;

pub fn add(left: u64, right: u64) -> u64 {
  left + right
}

#[cfg(test)]
mod tests {
  use std::io::Write;
  use rand::Rng;
  use crate::abstractions::IString;
  use crate::symbol::Symbol;
  use crate::dag_node::{DagNode, DagNodePtr, RootContainer};
  use crate::dag_node::allocator::acquire_allocator;
  use crate::util::{build_random_tree, print_tree};
  use super::*;

  #[test]
  fn test_symbols(){
    let symbols = (0..=10)
        .map(|x| {
          let name = IString::from(format!("symbol({})", x).as_str());
          Symbol::new(name, x)
        })
        .collect::<Vec<_>>();
    
    for symbol in symbols {
      println!("{}", symbol);
    }
  }
  
  #[test]
  fn test_dag_creation() {
    let symbols = (0..=10)
        .map(|x| {
          let name = IString::from(format!("sym({})", x).as_str());
          Symbol::new(name, x)
        })
        .collect::<Vec<_>>();

    let root = DagNode::new(&symbols[2]);
    let root_container = RootContainer::new(root);
    
    // Maximum tree height
    const max_height: usize = 6;
    const max_width : usize = 3;

    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();

    // Recursively build the random tree
    build_random_tree(&symbols, root, max_height, max_width);
    print_tree(root, String::new(), false);
    // println!("Symbols: {:?}", symbols);
    acquire_allocator().dump_memory_variables()
  }


  #[test]
  fn test_dag_lifecycle() {
    let symbols = (0..=10)
        .map(|x| {
          let name = IString::from(format!("sym({})", x).as_str());
          Symbol::new(name, x)
        })
        .collect::<Vec<_>>();

    let root = DagNode::new(&symbols[2]);
    let root_container = RootContainer::new(root);

    // Maximum tree height
    const max_height: usize = 7;
    const max_width : usize = 7;

    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();

    // Recursively build the random tree
    build_random_tree(&symbols, root, max_height, max_width);
    // print_tree(root, String::new(), false);
    // println!("Symbols: {:?}", symbols);
    acquire_allocator().dump_memory_variables()
  }

}
