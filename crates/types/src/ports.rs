//! A project's ports, or ranges of them, its walls may reach (4.1).

use serde::{Deserialize, Serialize};

/// A port, or a range of them, `"<from>-<to>"` (4.1): from 1 to 65535, `from` no more than `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    pub from: u16,
    pub to: u16,
}

impl Ports {
    /// `"<from>-<to>"`, or one port's number as text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let port = |t: &str| t.trim().parse::<u16>().ok().filter(|p| *p > 0);
        let (from, to) = match text.split_once('-') {
            Some((a, b)) => (port(a), port(b)),
            None => (port(text), port(text)),
        };
        match (from, to) {
            (Some(from), Some(to)) if from <= to => Ok(Self { from, to }),
            _ => Err(format!("{text:?} isn't a port (1 to 65535) or a range \"<from>-<to>\" with from <= to")),
        }
    }
}

impl Serialize for Ports {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.from == self.to {
            s.serialize_u16(self.from)
        } else {
            s.serialize_str(&format!("{}-{}", self.from, self.to))
        }
    }
}

impl<'de> Deserialize<'de> for Ports {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(i64),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Number(n) => Self::parse(&n.to_string()),
            Raw::Text(t) => Self::parse(&t),
        }
        .map_err(serde::de::Error::custom)
    }
}
