//! Passes over the document model that make one document of several, downstream of the
//! sources and upstream of the renderers, which see an ordinary document.

pub mod parallel;

pub use parallel::{Parallel, ParallelError, parallel};
