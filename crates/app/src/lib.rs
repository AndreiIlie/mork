//! The parts of the `mork` binary its tests drive as the binary does: the window's wiring to
//! the engine (`wiring`), and the live check (`live`).
#![forbid(unsafe_code)]

pub mod live;
pub mod wiring;
