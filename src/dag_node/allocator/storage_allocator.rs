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
  alloc::{alloc_zeroed, Layout},
  sync::{Mutex, MutexGuard}
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
  match GLOBAL_STORAGE_ALLOCATOR.try_lock() {
    Ok(allocator) => allocator,
    Err(e) => {
      panic!("Global storage allocator is deadlocked: {}", e);
    }
  }
}

pub struct StorageAllocator {
  // General settings
  show_gc   : bool, // Do we report GC stats to user
  early_quit: u64,  // Do we quit early for profiling purposes

  need_to_collect_garbage        : bool,

  // Bucket management variables
  bucket_count: u32,    // Total number of buckets
  bucket_list   : *mut Bucket, // Linked list of "in use" buckets
  unused_list   : *mut Bucket, // Linked list of unused buckets
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
      show_gc          : true,
      early_quit       : 0,

      need_to_collect_garbage: false,

      bucket_count: 0,
      bucket_list   : std::ptr::null_mut(),
      unused_list   : std::ptr::null_mut(),
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

    let mut b = unsafe { self.bucket_list.as_mut() };

    while let Some(bucket) = b {
      if bucket.bytes_free >= bytes_needed {
        bucket.bytes_free -= bytes_needed;
        let t = bucket.next_free;
        bucket.next_free = {
          // align next_free on 8 byte boundary
          let mut next_free = unsafe { t.add(bytes_needed) };
          let align_offset = next_free.align_offset(8);
          if align_offset == usize::MAX {
            panic!("Cannot align memory to 8 byte boundary")
          }
          bucket.bytes_free -= align_offset;
          next_free = unsafe { next_free.add(align_offset) };

          next_free
        };

        return t;
      }

      b = unsafe { bucket.next_bucket.as_mut() };
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
    let mut prev_bucket: *mut Bucket = std::ptr::null_mut();
    let mut bucket:      *mut Bucket = self.unused_list;
    loop{
      if bucket.is_null() {
        break;
      }
      let bucket_mut = bucket.as_mut_unchecked();
      if bucket_mut.bytes_free >= bytes_needed {
        // Move bucket from unused list to in use list
        if prev_bucket.is_null() {
          self.unused_list = bucket_mut.next_bucket;
        } else {
          prev_bucket.as_mut_unchecked().next_bucket = bucket_mut.next_bucket;
        }

        bucket_mut.next_bucket = self.bucket_list;
        self.bucket_list = bucket;

        // Allocate storage from bucket
        bucket_mut.bytes_free -= bytes_needed;
        let t = bucket_mut.next_free;
        bucket_mut.next_free = t.add(bytes_needed);
        return t;
      }

      prev_bucket = bucket;
      bucket = bucket_mut.next_bucket
    }

    // Create a new bucket.
    // ToDo: This should be a static method on Bucket.
    let mut size = BUCKET_MULTIPLIER * bytes_needed;
    size = size.max(MIN_BUCKET_SIZE);

    bucket = {
      let chunk: *mut u8 = alloc_zeroed(
        Layout::from_size_align(size, 8).unwrap()
      );
      chunk as *mut Bucket
    };

    self.bucket_count += 1;
    let t: *mut Void        = bucket.add(1) as *mut Void;
    let byte_count          = size - size_of::<Bucket>();

    self.total_bytes_allocated += byte_count;
    // Initialize the bucket
    let bucket_mut          = bucket.as_mut_unchecked();
    bucket_mut.nr_bytes     = byte_count;
    bucket_mut.bytes_free   = byte_count - bytes_needed;
    bucket_mut.next_free    = t.add(bytes_needed);
    // Put it at the head of the bucket linked list
    bucket_mut.next_bucket  = self.bucket_list;
    self.bucket_list        = bucket;

    t
  }

  /// Prepare bucket storage for mark phase of GC
  pub(crate) fn _prepare_to_mark(&mut self) {
    self.old_storage_in_use = self.storage_in_use;
    self.bucket_list        = self.unused_list;
    self.unused_list        = std::ptr::null_mut();
    self.storage_in_use     = 0;
    
    self.need_to_collect_garbage = false;
  }

  /// Garbage Collection for Buckets, called after mark completes
  pub(crate) unsafe fn _sweep_garbage(&mut self) {
    let mut bucket         = self.bucket_list;

    self.unused_list = bucket;
    while !bucket.is_null() {
      let bucket_mut        = bucket.as_mut_unchecked();
      bucket_mut.bytes_free = bucket_mut.nr_bytes;
      bucket_mut.next_free  = bucket.add(1) as *mut Void;
      bucket = bucket_mut.next_bucket;
    }
    self.target = max(self.target, TARGET_MULTIPLIER*self.storage_in_use);

    if self.show_gc {
      println!(
        "Buckets: {}\tBytes: {} ({:.2} MB)\tIn use: {} ({:.2} MB)\tCollected: {} ({:.2} MB)\tNow: {} ({:.2} MB)",
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

