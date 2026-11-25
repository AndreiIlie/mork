//! The boards (4.18): the engine reads and writes their files; their format, one parser and
//! writer with no I/O, is `core::board`, so `core` decides what a board write holds (M3.16).

pub use factory_core::board::*;
