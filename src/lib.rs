#![feature(ptr_as_ref_unchecked)]
#![allow(dead_code)]
extern crate core;

mod symbol;
mod abstractions;
mod dag_node;
mod util;


#[cfg(test)]
mod tests {
  use std::io::Write;
  use crate::{
    dag_node::{
      DagNode,
      DagNodePtr,
      RootContainer,
      allocator::acquire_node_allocator
    },
    symbol::Symbol,
    abstractions::IString,
    util::{build_random_tree, print_tree}
  };

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

}
