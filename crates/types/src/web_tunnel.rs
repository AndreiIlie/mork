//! The window's two web forwards (fac-35); SSH resolves each alias with Andrei's own config.

/// A web environment with a fixed loopback forward and browser address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Production,
    Staging,
}

impl Environment {
    pub const ALL: [Self; 2] = [Self::Production, Self::Staging];

    pub fn index(self) -> usize {
        match self {
            Self::Production => 0,
            Self::Staging => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
        }
    }

    pub fn alias(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
        }
    }

    pub fn port(self) -> u16 {
        match self {
            Self::Production => 9081,
            Self::Staging => 9082,
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Production => "http://localhost:9081/root",
            Self::Staging => "http://localhost:9082/root",
        }
    }
}

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
