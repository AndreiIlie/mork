//! Stand-ins for the outside world, for tests only (1.10): fake CLIs and services, the
//! probe, and the test home (1.10.7).
#![forbid(unsafe_code)]
// why: the fakes are test tooling (1.10); a panic there is the test's failure.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

pub mod door;
pub mod door_clients;
pub mod fake;
pub mod home;
pub mod live;
pub mod probe;
pub mod runlog;
pub mod scenario;

pub use home::TestHome;
