/*!

A `Bucket` is a small arena. We might use bumpalo or something instead.

*/

use crate::dag_node::Void;

pub struct Bucket {
  pub(crate) bytes_free : usize,
  pub(crate) next_free  : *mut Void,
  pub(crate) nr_bytes   : usize,
  pub(crate) next_bucket: *mut Bucket,
}

impl Default for Bucket {
  fn default() -> Self {
    Bucket {
      bytes_free : 0,
      next_free  : std::ptr::null_mut(),
      nr_bytes   : 0,
      next_bucket: std::ptr::null_mut(),
    }
  }
}