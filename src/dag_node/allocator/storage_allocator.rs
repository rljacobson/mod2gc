/*!

# Bucket Allocator

See GarbageCollector.md for a detailed explanation of how it works. Below is a brief summary of how it works.

The Bucket allocator manages memory by organizing it into buckets, each containing raw memory that can be allocated in smaller chunks. When a program requests memory, the allocator first searches the in-use buckets for a free chunk. In the typical case, the current active bucket has the capacity to allocate the requested chunk, and so the allocator acts as a "bump" allocator. If no suitable space is found, it checks unused buckets (if any exist) or allocates new ones to accommodate the request.

The garbage collection process in the bucket allocator follows a mark-and-sweep pattern with a copying strategy. During the mark phase, the allocator traverses the live data and copies it to available initially empty buckets (i.e. buckets which were empty prior to garbage collection). If the available buckets do not have enough space to accommodate the live objects, new buckets are allocated and added to the list. Once the objects are copied, the old memory locations are free to be collected in the sweep phase.

In the sweep phase, the allocator clears the old buckets, resetting their free space to the full bucket size. These buckets are then moved to the unused list and reset to an empty state, making them available for future allocations.

Because live objects are relocated during garbage collection to previously empty buckets, there is no fragmentation after garbage collection. What's more, copying occurs in depth-first order on the graph nodes, improving locality for certain access patterns.

*/

use std::{
  cmp::max,
  sync::{Mutex, MutexGuard},
  ptr::NonNull
};

use once_cell::sync::Lazy;

use crate::{
  dag_node::{
    allocator::bucket::Bucket,
    Void
  }
};


const BUCKET_MULTIPLIER    : usize = 8;              // To determine bucket size for huge allocations
const MIN_BUCKET_SIZE      : usize = 256 * 1024 - 8; // Bucket size for normal allocations
const INITIAL_TARGET       : usize = 220 * 1024;     // Just under 8/9 of MIN_BUCKET_SIZE
const TARGET_MULTIPLIER    : usize = 8;

static GLOBAL_STORAGE_ALLOCATOR: Lazy<Mutex<StorageAllocator>> = Lazy::new(|| {
  Mutex::new(StorageAllocator::new())
});


pub fn acquire_storage_allocator()  -> MutexGuard<'static, StorageAllocator> {
  GLOBAL_STORAGE_ALLOCATOR.lock().unwrap()
}

pub struct StorageAllocator {
  // General settings
  show_gc   : bool, // Do we report GC stats to user
  early_quit: u64,  // Do we quit early for profiling purposes

  need_to_collect_garbage: bool,

  // Bucket management variables
  bucket_count  : u32,    // Total number of buckets
  bucket_list   : Option<NonNull<Bucket>>, // Linked list of "in use" buckets
  unused_list   : Option<NonNull<Bucket>>, // Linked list of unused buckets
  storage_in_use: usize,  // Amount of bucket storage in use (bytes)
  total_bytes_allocated: usize,  // Total amount of bucket storage (bytes)
  old_storage_in_use   : usize, // A temporary to remember storage use prior to GC.
  target        : usize,  // Amount to use before GC (bytes)
}

// Access is hidden behind a mutex.
unsafe impl Send for StorageAllocator {}
// unsafe impl Sync for Allocator {}

impl StorageAllocator {
  pub fn new() -> Self {
    StorageAllocator {
      show_gc       : true,
      early_quit    : 0,

      need_to_collect_garbage: false,

      bucket_count: 0,
      bucket_list   : None,
      unused_list   : None,
      storage_in_use: 0,
      total_bytes_allocated: 0,
      old_storage_in_use   : 0,
      target        : INITIAL_TARGET,
    }
  }

  /// Query whether the allocator has any garbage to collect.
  #[inline(always)]
  pub fn want_to_collect_garbage(&self) -> bool {
    self.need_to_collect_garbage
  }

  /// Allocates the given number of bytes using bucket storage.
  pub fn allocate_storage(&mut self, bytes_needed: usize) -> *mut Void {
    assert_eq!(bytes_needed % size_of::<usize>(), 0, "only whole machine words can be allocated");
    self.storage_in_use += bytes_needed;

    if self.storage_in_use > self.target {
      self.need_to_collect_garbage = true;
    }

    let mut b = self.bucket_list;

    while let Some(mut bucket) = b {
      let bucket = unsafe{ bucket.as_mut() };

      if bucket.bytes_free >= bytes_needed {
        return bucket.allocate(bytes_needed);
      }

      b = bucket.next_bucket;
    }

    // No space in any bucket, so we need to allocate a new one.
    unsafe{ self.slow_allocate_storage(bytes_needed) }
  }

  /// Allocates the given number of bytes by creating more bucket storage.
  unsafe fn slow_allocate_storage(&mut self, bytes_needed: usize) -> *mut u8 {
    #[cfg(feature = "gc_debug")]
    {
      eprintln!("slow_allocate_storage()");
    }
    // Loop through the bucket list
    let mut prev_bucket: Option<NonNull<Bucket>> = None;
    let mut maybe_bucket = self.unused_list;
    
    while let Some(mut bucket) = maybe_bucket {
      let bucket_mut = bucket.as_mut();
      
      if bucket_mut.bytes_free >= bytes_needed {
        // Move bucket from unused list to in use list
        
        if let Some(mut prev_bucket) = prev_bucket {
          prev_bucket.as_mut().next_bucket = bucket_mut.next_bucket;
        } else {
          self.unused_list = bucket_mut.next_bucket;
        }

        bucket_mut.next_bucket = self.bucket_list;
        self.bucket_list       = maybe_bucket;

        // Allocate storage from bucket
        return bucket_mut.allocate(bytes_needed);
      }

      prev_bucket  = maybe_bucket;
      maybe_bucket = bucket_mut.next_bucket
    }

    // Create a new bucket.
    // ToDo: This should be a static method on Bucket.
    let mut size = BUCKET_MULTIPLIER * bytes_needed;
    size         = size.max(MIN_BUCKET_SIZE);

    let mut new_bucket = Bucket::with_capacity(size);
    let t              = new_bucket.allocate(bytes_needed);

    self.bucket_count          += 1;
    self.total_bytes_allocated += size;
   
    // Put it at the head of the bucket linked list
    new_bucket.next_bucket = self.bucket_list;
    self.bucket_list       = Some(NonNull::new_unchecked(Box::into_raw(Box::new(new_bucket))));

    t
  }

  /// Prepare bucket storage for mark phase of GC
  pub(crate) fn _prepare_to_mark(&mut self) {
    self.old_storage_in_use = self.storage_in_use;
    self.bucket_list        = self.unused_list;
    self.unused_list        = None;
    self.storage_in_use     = 0;

    self.need_to_collect_garbage = false;
  }

  /// Garbage Collection for Buckets, called after mark completes
  pub(crate) unsafe fn _sweep_garbage(&mut self) {
    let mut maybe_bucket = self.bucket_list;

    // Reset all formerly active buckets
    self.unused_list = maybe_bucket;
    while let Some(mut bucket) = maybe_bucket {
      let bucket_mut = bucket.as_mut();
      bucket_mut.reset();
      maybe_bucket = bucket_mut.next_bucket;
    }
    self.target = max(self.target, TARGET_MULTIPLIER*self.storage_in_use);

    if self.show_gc {
      println!(
        "{:<10} {:<10} {:<10} {:<10} {:<13} {:<10} {:<10} {:<10} {:<10}",
        "Buckets",
        "Bytes",
        "Size (MB)",
        "In use",
        "In use (MB)",
        "Collected",
        "Col. (MB)",
        "Now",
        "Now (MB)"
      );
      println!(
        "{:<10} {:<10} {:<10.2} {:<10} {:<13.2} {:<10} {:<10.2} {:<10.2}  {:<10.2}",
        self.bucket_count,
        self.total_bytes_allocated,
        (self.total_bytes_allocated as f64) / (1024.0 * 1024.0),
        self.old_storage_in_use,
        (self.old_storage_in_use as f64) / (1024.0 * 1024.0),
        self.old_storage_in_use - self.storage_in_use,
        ((self.old_storage_in_use - self.storage_in_use) as f64) / (1024.0 * 1024.0),
        self.storage_in_use,
        (self.storage_in_use as f64) / (1024.0 * 1024.0),
      );
    }

  }

}

