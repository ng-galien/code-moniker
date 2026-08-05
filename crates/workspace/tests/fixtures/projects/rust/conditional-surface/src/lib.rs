mod safe;

#[cfg(feature = "fast")]
pub use fast_backend::Backend;

#[cfg(not(feature = "fast"))]
pub use safe::Backend;
