# mod2gc Garbage Collecting Allocator for Graph Nodes

This is actually two garbage collecting allocators:

1. An arena allocator exclusively for allocating `DagNode` objects. All garbage collected nodes must be allocated with this allocator.
2. A "bucket" allocator exclusively for allocating any memory owned by `DagNode` objects. Nodes may have several arguments, which are other nodes. The arguments are stored as arrays of pointers to the argument nodes, and nodes must allocate these arrays of pointers using the bucket allocator and hold on to a pointer to the array.

## Usage

There are only three free functions in the public API: 

 1. `ok_to_collect_garbage()`: Indicates a safe point where the allocator can collect garbage if it needs to. 
 2. `want_to_collect_garbage()`: Queries the allocator whether it needs to collect garbage. 
 3. `allocate_dag_node()`: Does what it says on the tin.

A garbage collection event invalidates any node argument reference or iterator, so `ok_to_collect_garbage` should 
only be called at times when there are none. 

# Authorship and License

Copyright © 2024 Robert Jacobson. This software is distributed under the MIT license or Apache 2.0 license at your
preference.
