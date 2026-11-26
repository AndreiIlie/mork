//! A running suite's progress, read from its output one line at a time (fac-697): xtask's results
//! as they land and its passes' test counts, or the test files Jest's default reporter names.

use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use mork_core::queue::failing::{path, unstyled};
use mork_core::queue::suite_log::{JestMarker, XTASK, jest_marker, live_result};
use mork_types::view::SuiteProgress;

/// A suite run's output file as read so far.
#[derive(Debug, Default)]
pub struct Output {
    reader: Reader,
    offset: u64,
    // Held open with its inode: the install's file the runner unlinks keeps its number while held,
    // so the suite's file made after it has another.
    file: Option<(std::fs::File, u64)>,
}

impl Output {
    /// Reads the complete lines `file` gained since the last read, from its start when it was made anew.
    pub fn more(&mut self, file: &Path) {
        let Ok(now) = std::fs::metadata(file).map(|m| m.ino()) else { return };
        if self.file.as_ref().is_none_or(|(_, held)| *held != now) {
            let Ok(f) = std::fs::File::open(file) else { return };
            let Ok(ino) = f.metadata().map(|m| m.ino()) else { return };
            *self = Self { file: Some((f, ino)), ..Self::default() };
        }
        let Some((f, _)) = self.file.as_mut() else { return };
        let mut buf = Vec::new();
        if f.seek(SeekFrom::Start(self.offset)).is_err() || f.read_to_end(&mut buf).is_err() {
            return;
        }
        // The run may be writing its last line.
        let complete = buf.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
        for line in String::from_utf8_lossy(&buf[..complete]).lines() {
            self.reader.line(line);
        }
        self.offset += complete as u64;
    }

    /// Lets the file go once nothing more will be read of it, keeping what it counted.
    pub fn close(&mut self) {
        self.file = None;
    }

    /// What its output counted so far; none until it said something countable.
    pub fn progress(&self) -> Option<SuiteProgress> {
        self.reader.progress()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Xtask,
    Jest,
}

/// A suite's output as read so far.
#[derive(Debug, Default)]
pub struct Reader {
    format: Option<Format>,
    counted: SuiteProgress,
    // A build's lines are no progress: none shows until something countable is said.
    seen: bool,
    // A pass of xtask's that started without its count leaves the suite's total unknown.
    uncounted: bool,
    // Jest's summary repeats the files that failed.
    summary: bool,
}

impl Reader {
    /// Reads one line of the suite's output.
    pub fn line(&mut self, line: &str) {
        let plain = unstyled(line);
        if let Some(said) = plain.strip_prefix(XTASK) {
            if self.format.get_or_insert(Format::Xtask) == &Format::Xtask {
                self.xtask(said);
            }
        } else if self.format != Some(Format::Xtask) && !self.summary {
            self.jest(&plain);
        }
    }

    /// What it counted so far; none until its output said something countable.
    pub fn progress(&self) -> Option<SuiteProgress> {
        let known = self.format == Some(Format::Xtask) && !self.uncounted;
        self.seen.then(|| SuiteProgress { total: self.counted.total.filter(|_| known), ..self.counted.clone() })
    }

    // A pass's count, `<what>: <k> test binaries, <n> tests, <m> at a time`, and each test's
    // result, `<binary>: test <name> ... <result>`. The binaries' own output, printed whole, is not read.
    fn xtask(&mut self, said: &str) {
        if let Some((name, result)) = live_result(said) {
            let passed = result == "ok" || result == "ignored" || result.starts_with("ignored, ");
            if passed || result == "FAILED" {
                self.ended(name, passed);
            }
            return;
        }
        let Some((_, said)) = said.split_once(": ") else { return };
        let mut parts = said.split(", ");
        if !parts.next().is_some_and(|p| p.ends_with(" test binaries")) {
            return;
        }
        self.seen = true;
        match parts.find_map(|p| p.strip_suffix(" tests")?.parse::<u32>().ok()) {
            Some(n) => self.counted.total = Some(self.counted.total.unwrap_or(0).saturating_add(n)),
            None => self.uncounted = true,
        }
    }

    // Jest heads each test file as it ends `PASS <path>` or `FAIL <path>`.
    fn jest(&mut self, line: &str) {
        let (rest, passed) = match jest_marker(line) {
            Some(JestMarker::Pass(rest)) => (rest, true),
            Some(JestMarker::Fail(rest)) => (rest, false),
            Some(JestMarker::Summary | JestMarker::Totals) => {
                self.summary = true;
                return;
            }
            None => return,
        };
        let Some(file) = path(rest) else { return };
        self.format = Some(Format::Jest);
        self.counted.files = true;
        self.ended(&file, passed);
    }

    fn ended(&mut self, name: &str, passed: bool) {
        let n = if passed { &mut self.counted.passed } else { &mut self.counted.failed };
        *n = n.saturating_add(1);
        self.counted.latest = Some(name.to_owned());
        self.seen = true;
    }
}
