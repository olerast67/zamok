#![deny(unsafe_op_in_unsafe_fn)]
mod fs;
pub mod gate;
mod native;

pub use fs::context::NtPassthroughContext;
pub use fs::file::NtPassthroughFile;
pub use fs::ntptfs::NtPassthroughFilesystem;
