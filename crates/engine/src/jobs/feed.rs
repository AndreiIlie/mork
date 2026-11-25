//! A live turn's stdin (fac-389): its supervisor speaks the prompt, then each message delivered into its folder, as
//! the turn's stream asks. Nothing here waits: the supervisor's one thread goes on watching its job.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::process::ChildStdin;

use factory_types::job::JobSpec;

use super::folder::JobDir;
use crate::cli::live::{Delivery, Driver};
use crate::files;

/// What a live turn's stdin is still to be told.
pub struct Feed {
    driver: Driver,
    pipe: Option<ChildStdin>,
    pending: Vec<u8>,
    // A line partly written is finished even once the driver is done: a torn line would fail the turn.
    mid_line: bool,
    read_at: u64,
    partial: Vec<u8>,
    sent: BTreeSet<u64>,
}

impl Feed {
    /// The feed of a job whose spec speaks live input, its opening lines waiting; none for any other job.
    pub fn new(job: &JobDir, spec: &JobSpec) -> io::Result<Option<Self>> {
        let Some(live) = &spec.live else { return Ok(None) };
        let prompt = std::fs::read_to_string(job.stdin())?;
        let schema = files::read_json::<serde_json::Value>(&job.io().join("schema.json")).ok().flatten();
        let (driver, lines) = Driver::start(live, &prompt, schema);
        let mut feed = Self {
            driver,
            pipe: None,
            pending: Vec::new(),
            mid_line: false,
            read_at: 0,
            partial: Vec::new(),
            sent: BTreeSet::new(),
        };
        feed.queue(lines);
        Ok(Some(feed))
    }

    /// The turn's stdin, once it started; the pipe dropped, the turn reads its end.
    pub fn attach(&mut self, pipe: Option<ChildStdin>) {
        let Some(pipe) = pipe else { return };
        match factory_sys::set_nonblocking(pipe.as_raw_fd()) {
            Ok(()) => self.pipe = Some(pipe),
            Err(e) => eprintln!("supervisor: the turn's stdin can't be written without waiting: {e}"),
        }
    }

    /// One pass: the stream's new lines answered, new messages handed on, what waits written as far as the pipe
    /// takes it; the turn's stdin closed once the driver is done, and no line is half written.
    pub fn step(&mut self, job: &JobDir) {
        if self.pipe.is_none() {
            return;
        }
        for line in self.new_lines(job) {
            let out = self.driver.output(&line);
            self.queue(out);
        }
        for (seq, d) in self.delivered(job) {
            self.sent.insert(seq);
            let out = d.map(|d| self.driver.deliver(d)).unwrap_or_default();
            self.queue(out);
        }
        if self.driver.done() {
            // Whatever the turn would read after its end is for its next turn's news, not this one.
            let rest =
                if self.mid_line { self.pending.iter().position(|b| *b == b'\n').map_or(0, |i| i + 1) } else { 0 };
            self.pending.truncate(rest);
        }
        self.write();
        if self.driver.done() && self.pending.is_empty() {
            self.pipe = None;
        }
    }

    fn queue(&mut self, lines: Vec<String>) {
        for line in lines {
            self.pending.extend_from_slice(line.as_bytes());
            self.pending.push(b'\n');
        }
    }

    fn write(&mut self) {
        let Some(pipe) = self.pipe.as_mut() else { return };
        while !self.pending.is_empty() {
            match pipe.write(&self.pending) {
                Ok(n) => {
                    self.mid_line = self.pending.get(..n).and_then(<[u8]>::last).map_or(self.mid_line, |b| *b != b'\n');
                    self.pending.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                // The turn closed its stdin: nothing more reaches it.
                Err(_) => {
                    self.pipe = None;
                    self.pending.clear();
                    return;
                }
            }
        }
    }

    // The stream's lines completed since the last pass.
    fn new_lines(&mut self, job: &JobDir) -> Vec<String> {
        let mut bytes = Vec::new();
        let read = File::open(job.out()).and_then(|mut f| {
            f.seek(SeekFrom::Start(self.read_at))?;
            f.read_to_end(&mut bytes)
        });
        let Ok(n) = read else { return Vec::new() };
        self.read_at += n as u64;
        self.partial.extend_from_slice(&bytes);
        let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') else { return Vec::new() };
        let whole: Vec<u8> = self.partial.drain(..=end).collect();
        String::from_utf8_lossy(&whole).lines().map(str::to_owned).collect()
    }

    // The messages written into its `input/` since, in their order; a file being written is named with a dot, and
    // one written whole that doesn't read never will.
    fn delivered(&self, job: &JobDir) -> Vec<(u64, Option<Delivery>)> {
        let Ok(entries) = std::fs::read_dir(job.input()) else { return Vec::new() };
        let mut seqs: Vec<u64> = entries
            .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".json")?.parse().ok())
            .filter(|s| !self.sent.contains(s))
            .collect();
        seqs.sort_unstable();
        seqs.into_iter().map(|s| (s, files::read_json(&job.input().join(format!("{s}.json"))).ok().flatten())).collect()
    }
}
