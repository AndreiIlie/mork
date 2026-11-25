//! When the engine read each part of a job's stream (fac-206), kept beside it: one line per read,
//! the stream's length then and the time, `-` for what was there before any read was stamped.

use std::io::Write;
use std::path::PathBuf;

/// A job's reads, as its stamps file keeps them.
#[derive(Debug, Clone, Default)]
pub struct Stamps {
    path: Option<PathBuf>,
    reads: Vec<(u64, Option<i64>)>,
}

impl Stamps {
    /// The reads its file holds; none when it has none, as a job from before fac-206.
    pub fn load(path: PathBuf) -> Self {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let reads = text
            .lines()
            .filter_map(|l| {
                let (end, at) = l.split_once(' ')?;
                Some((end.parse().ok()?, if at == "-" { None } else { Some(at.parse().ok()?) }))
            })
            .collect();
        Self { path: Some(path), reads }
    }

    /// The reads of a stream being watched: what it held already with no read stamped, as when
    /// the engine took over a job of a build before fac-206, is marked unstamped.
    pub fn watching(path: PathBuf, held: u64) -> Self {
        let mut s = Self::load(path);
        if s.reads.is_empty() && held > 0 {
            s.push(held, None);
        }
        s
    }

    /// When the line of the stream ending at `end` was read, if a stamped read took it.
    pub fn at(&self, end: u64) -> Option<i64> {
        let i = self.reads.partition_point(|(e, _)| *e < end);
        self.reads.get(i).and_then(|(_, at)| *at)
    }

    /// When the stream was last read, if that read was stamped.
    pub fn last(&self) -> Option<i64> {
        self.reads.last().and_then(|(_, at)| *at)
    }

    /// Whether a read already took the stream up to `end`.
    pub fn covers(&self, end: u64) -> bool {
        self.reads.last().is_some_and(|(e, _)| *e >= end)
    }

    /// The stream was read up to `end` at `at`: kept, once per length.
    pub fn read_to(&mut self, end: u64, at: i64) {
        if !self.covers(end) {
            self.push(end, Some(at));
        }
    }

    // A stamp that can't be written is a time the view goes without.
    fn push(&mut self, end: u64, at: Option<i64>) {
        self.reads.push((end, at));
        let Some(path) = &self.path else { return };
        let line = format!("{end} {}\n", at.map_or_else(|| "-".to_owned(), |a| a.to_string()));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
