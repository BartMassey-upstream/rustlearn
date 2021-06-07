//! Datasets and dataset loading utilities.

pub mod boston;
pub mod iris;

#[cfg(test)]
#[cfg(any(feature = "all_tests", feature = "bench"))]
pub mod newsgroups;
