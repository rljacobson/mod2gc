/*
Utilities to build and print random trees

*/

use rand::Rng;

use crate::{
  dag_node::{
    DagNode,
    DagNodePtr,
  },
  symbol::Symbol
};

/*
Recursively builds a random tree of `DagNode`s with a given height and arity rules.

Because this function holds on to iterators of `NodeVec`s, the GC cannot run during
the building of the tree. Run the GC before or after. 

 - `symbols`: List of `Symbol` objects of each arity from 0 to `max_width`.
 - `parent`: Pointer to the current parent node.
 - `max_height`: Maximum allowed height for the tree.
*/
pub fn build_random_tree(
  symbols   : &[Symbol],
  parent    : DagNodePtr,
  max_height: usize,
  max_width : usize,
  min_width : usize,
) {
  if max_height == 0 {
    return; // Reached the maximum depth
  }
  
  // idiot-proof
  let min_width = std::cmp::min(max_width, min_width);
  let max_width = std::cmp::max(max_width, min_width);
  
  let mut rng   = rand::thread_rng();

  // Get the parent node's arity from its symbol
  let parent_arity = unsafe { (*parent).arity() };

  // For each child based on the parent's arity, create a new node
  for i in 0..parent_arity as usize {
    // Determine the arity of the child node
    let child_arity = if max_height == 1 {
      0 // Leaf nodes must have arity 0
    } else {
      rng.gen_range(min_width..=max_width) // Random arity between min_width and max_width
    };

    // Create the child node with the symbol corresponding to its arity
    let child_symbol = &symbols[child_arity];
    let child_node   = DagNode::new(child_symbol);

    // Insert the child into the parent node
    let parent_mut = unsafe{ parent.as_mut_unchecked() };
    if let Err(msg) = parent_mut.insert_child(child_node) {
      eprintln!("Failed to insert child: level = {} child = {} parent_arity = {}\n\t::{}", max_height, i, parent_arity, msg);
    };

    // Recursively build the subtree for the child
    build_random_tree(symbols, child_node, max_height - 1, max_width, min_width);
  }
}

/// Recursively prints a tree structure using ASCII box-drawing symbols.
///
/// - `node`: The current node to print.
/// - `prefix`: The string prefix to apply to the current node's line.
/// - `is_tail`: Whether the current node is the last child of its parent.
pub fn print_tree(node: DagNodePtr, prefix: String, is_tail: bool) {
  assert_ne!(node, std::ptr::null_mut());
  let is_head = prefix.is_empty();

  let node: &DagNode = unsafe{ &*node };

  // Print the current node
  let new_prefix = if is_head {
    ""
  }else {
    if is_tail { "╰──" } else { "├──" }
  };
  println!(
    "{}{}{}",
    prefix,
    new_prefix,
    node
  );

  // Determine the new prefix for children
  let new_prefix = if is_tail {
    format!("{}    ", prefix)
  } else if is_head {
    format!(" ")
  }
  else {
    format!("{}│   ", prefix)
  };

  // Print each child
  for (i, &child_ptr) in node.iter_children().enumerate() {
    print_tree(
      child_ptr,
      new_prefix.clone(),
      i == node.len() - 1, // Is this the last child?
    );
  }
}
