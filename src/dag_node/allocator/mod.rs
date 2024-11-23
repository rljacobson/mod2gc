/*!
The allocator for garbage collected memory. This is really two different allocators which collect garbage at the same time:

 1. An arena allocator exclusively for allocating `DagNode` objects. All garbage collected nodes must be allocated with this allocator.
 2. A "bucket" allocator exclusively for allocating any memory owned by `DagNode` objects. Nodes may have several arguments, which are other nodes. The arguments are stored as arrays of pointers to the argument nodes, and nodes must allocate these arrays of pointers using the bucket allocator and hold on to a pointer to the array.


*/

mod arena;
mod bucket;
pub mod node_vector;
mod node_allocator;
mod storage_allocator;

pub use node_allocator::{acquire_node_allocator, increment_active_node_count};
#[allow(unused_imports)]
pub use node_allocator::NodeAllocator;


