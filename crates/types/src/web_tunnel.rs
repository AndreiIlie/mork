//! The window's web forwards (fac-35, fac-721); SSH resolves each configured alias with Andrei's own config.

/// The owned SSH connection's state; an open connection does not prove the remote app's health.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Closed,
    Opening,
    Open,
    Closing,
    Failed(String),
    Dropped(String),
}
