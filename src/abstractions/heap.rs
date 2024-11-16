/*!

# Heap Helpers for Manual Memory Management in Rust

## Overview

This module provides two macros, `heap_construct` and `heap_destroy`, that facilitate the manual creation and destruction of heap-allocated objects in Rust. These macros mimic C-style memory management, where the user is responsible for managing the lifecycle of dynamically allocated memory. This is particularly useful when you need explicit control over object lifetimes in environments where automatic memory management (such as Rust's RAII) isn't suitable or possible.

- **`heap_construct!`:** Creates a heap-allocated object and returns a raw pointer (`*mut T`) to it, bypassing Rust's automatic memory management. The user takes responsibility for manually freeing the memory.

- **`heap_destroy!`:** Reclaims the memory associated with a raw pointer returned by `heap_construct!`. It converts the raw pointer back into a `Box<T>`, which is then dropped allowing Rust to deallocate the memory.

Because both macros use raw pointers, they are inherently **unsafe**, and it is up to the user to ensure safety by following strict memory management rules, such as avoiding double frees, preventing use-after-free, and ensuring no aliasing of mutable references.

## Usage Example

Here’s how you might use `heap_construct!` and `heap_destroy!`:

```ignore
use heap::heap_construct;
use heap::heap_destroy;

// Create a heap-allocated integer and obtain a raw pointer to it
let ptr: *mut i32 = heap_construct!(42);

unsafe {
    // Dereference the raw pointer to access the value
    println!("Value: {}", *ptr);
}

// Safely destroy the heap-allocated object, reclaiming the memory
heap_destroy!(ptr);
```

## Safety Considerations

Both macros rely on raw pointers, making them unsafe. These macros bypass Rust’s typical ownership and borrowing rules to give you more control over memory management. As a result, you should carefully manage the lifetimes of the pointers you create and destroy. In particular:

 - **No Double-Free:** Ensure that the same pointer is not freed twice.
 - **No Use-After-Free:** Once `heap_destroy!` has been called on a pointer, any attempt to dereference or use that pointer is undefined behavior.
 - **No Aliased Mutable Pointers:** Ensure that no other code holds a mutable or immutable reference to the object when
    you call `heap_destroy!`.


## Use Cases

This module is useful in scenarios where you need to:

 - Interface with low-level systems or external APIs that expect raw pointers and manual memory management.
 - Explicitly control memory lifetimes in performance-critical sections of code where Rust’s automatic memory management may be undesirable.
 - Work with unsafe or FFI (Foreign Function Interface) code, such as when interoperating with C libraries.

*/


/// Construct a new mutable pointer to a new heap allocated object. This is obviously
/// an unsafe operation. It is up to the user to manually destroy the object and
/// reclaim the memory. The `heap_destroy` macro is provided for this purpose.
#[macro_export]
macro_rules! heap_construct {
    ($expr:expr) => {{
        // Use Box::new to create the object on the heap
        let boxed = Box::new($expr);
        // Convert the Box into a raw pointer, transferring ownership
        // and thus preventing automatic deallocation
        Box::into_raw(boxed)
    }};
}
#[allow(unused_imports)]
pub use heap_construct;

/// Destroy a heap allocated object pointed to by a mutable pointer. This is
/// the companion macro to `heap_construct`. It is up to the user to ensure
/// no use after free, no aliasing pointers, and all other safety checks.
#[macro_export]
macro_rules! heap_destroy {
    ($ptr:expr) => {{
        // Assert that the given expression is a mutable raw pointer to prevent misuse.
        // This line does nothing at runtime but ensures type correctness.
        let _ = $ptr as *mut _;

        // Convert the raw pointer back into a Box, taking ownership back
        // and enabling Rust's automatic memory management
        unsafe { Box::from_raw($ptr); }
        // The Box is dropped here, and the memory is deallocated
    }};
}
#[allow(unused_imports)]
pub use heap_destroy;