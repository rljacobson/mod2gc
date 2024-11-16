mod heap;


// Interned string. Use `DefaultAtom` for a global cache that can be used across threads. Use `Atom` for a thread-local
// string cache.
pub use string_cache::DefaultAtom as IString;
