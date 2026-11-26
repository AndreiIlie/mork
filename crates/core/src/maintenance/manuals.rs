//! Sessions receive rules at startup and changes on reload; command details stay on demand
//! so procedures do not inflate every prompt (fac-14, fac-88).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mork_types::catalog::{CATALOG, find, signature};
use mork_types::reply::Role;
use serde::Serialize;

use crate::search_text::{excerpt, fold};

/// The shared Mork manual: every role's required reading.
pub const MORK: &str = include_str!("../../../../manuals/mork.md");

// The technical reference's text: its introduction, then one `### <name>` section per command
// of the catalog and per helper. Signatures are the catalog's, put in when it is shown.
const REFERENCE: &str = include_str!("../../../../manuals/reference.md");

/// Local command listed beside the engine's catalog commands.
pub const SEARCH_SIGNATURE: &str = "mork ctl search <query> [--json]";

/// The reply contract, whole at every session's start (3.4.6).
pub const CONTRACT: &str = "Reply with one JSON object, as your schema says: `hand`, what the operator reads (its `color` grey, green, yellow or red; a one-line `headline`; a `message`, the complete text for the operator when you ask him nothing, empty or one line beside an ask to him, which he reads alone; `ask`, what you need from the operator, the one way you address him, your turn then ending to wait for his answer, or empty: a non-empty one is an ask, a child's is a request to its parent, its own words only; `options`, the ask's choices, one line each, which the engine numbers; `pick`, the number of the option you would pick, and `reason`, its why; the planner or foreman's `changes` and `code_shape`, a recap's sketch, one plain line each; and the planner or foreman's `design`, true when that ask is a design question, which tags it \"design\"; an open ask stays open through later replies that ask nothing, until it is answered or replaced by a new `ask`, or your `withdraw_ask` true withdraws it, which a child lacks; and `images`, image files of your `io/` folder, `$MORK_IO`, each by its full path, shown beside the hand and its ask); `messages`, each a `to`, a `kind`, a `text` and its `images`, image files of your `io/` folder, `$MORK_IO` that travel with it into its recipient's turn, its kind `request` (it wants a response), `response` (it answers a request) or `update` (it informs), a response and an update wanting nothing back: one that reaches an agent wakes it, whatever its kind, and one to a recipient Mork can't place, the operator and a room included, is not delivered, its sender told so, and a full card's foreman's to `planner` and its planner's to `foreman` land in its challenge place, which the window shows nowhere; `findings`, each a `file`, a `line`, a `summary` and a `proof`; `risks`; `outcome`, one of your role's outcomes; and `schema`. Every property your schema names is always present, whether or not it has anything to say: `ask` an empty string when you ask nothing, `options` empty, `pick` 0 and `reason` empty when it offers no choice, the planner or foreman's `changes` and `code_shape` empty outside a recap, the planner or foreman's `design` false when it asks no design question, `withdraw_ask` (a child has none) false when it withdraws nothing, `images`, of the hand and of each message, empty when you attach none, `messages`, `findings` and `risks` empty arrays when there are none. Six things refuse a reply: it doesn't parse, its outcome isn't one of yours, a finding lacks its file, line or proof, a message lacks its kind, an image isn't a regular file of your `io/` folder, `$MORK_IO` (a link, another folder's file, a folder or a missing file is refused), or the outcome of a full card's planner or foreman, or a report card's foreman, has no effect while its card waits on its reply in its room and nothing else would bring it a turn (`working` and `blocked` are never refused). That last refusal names where the card stands and which outcome it waits for. A refused reply posts neither its hand nor its messages: write them again in the correction.";

/// What a resumed turn is reminded of the contract (fac-14): its whole text came at the session's
/// start, or since in an announcement.
pub const REMINDER: &str =
    "Reply with one JSON object, as your schema and the latest reply contract this session was given say.";

/// A planner's and a foreman's reminder of the ask form when they resume (fac-529, fac-596, fac-608): the operator's output
/// style wins where it differs.
pub const FOREMAN_REMINDER: &str = "When your system prompt carries the operator's output style, it takes precedence wherever it differs from the ask form. A decision ask to the operator stands alone, never opening with a list of decisions still to come or a recap. Its `ask` is a few lines, in this order: only what he must know for this one; one question of at most 25 words, answerable with yes, no or a number. Two or three options when it has any go in `options`, one line each, which the engine numbers; your pick's number in `pick` and its why in `reason`, never in the ask's text. Every line but the question is at most 15 words. Never open by acknowledging the previous decision, put nothing of the kind between headline and question, and never repeat the headline in the body. An answer to his question is one or two plain sentences, detail only when he asks.";

// The sections a researcher's start leaves out of the Mork manual, by heading (fac-624, decision 1): a renamed
// heading is sent, erring toward more.
const NOT_FOR_RESEARCHERS: [&str; 6] = [
    "Who is here",
    "Places",
    "A card's life",
    "The card is the memory",
    "Who may do what",
    "Verification and the merge queue",
];

/// The Mork manual as a seat of `role` starts with it: whole, but for a researcher, which one question needs
/// less of.
pub fn mork_for(role: Role) -> String {
    if role != Role::Researcher {
        return MORK.to_owned();
    }
    let mut out = String::new();
    let mut kept = true;
    for line in MORK.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            kept = !NOT_FOR_RESEARCHERS.contains(&heading.trim_end());
        }
        if kept {
            out.push_str(line);
        }
    }
    out
}

// The roles with a manual, in the order they are listed.
const ROLES: [Role; 5] = [Role::Planner, Role::Foreman, Role::Reviewer, Role::Researcher, Role::Baseline];

/// A role's manual (3.2), built into every build.
pub fn role_file(role: Role) -> &'static str {
    match role {
        Role::Planner => include_str!("../../../../roles/planner.md"),
        Role::Foreman => include_str!("../../../../roles/foreman.md"),
        Role::Reviewer => include_str!("../../../../roles/reviewer.md"),
        Role::Researcher => include_str!("../../../../roles/researcher.md"),
        Role::Baseline => include_str!("../../../../roles/baseline.md"),
        Role::Echo => "",
    }
}

// One section of the reference: a command's or a helper's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Section {
    // The command's name (`card.change`), or the helper's words (`mork test`).
    name: &'static str,
    // Its first paragraph: the API index's line.
    summary: &'static str,
    // Everything under its heading.
    body: &'static str,
}

// The reference's sections, in its order.
fn sections() -> impl Iterator<Item = Section> {
    REFERENCE.split("\n## ").flat_map(|group| group.split("\n### ").skip(1)).map(|entry| {
        let (name, tail) = entry.split_once('\n').unwrap_or((entry, ""));
        let body = tail.trim();
        let summary = body.split("\n\n").next().unwrap_or("").trim();
        Section { name: name.trim(), summary, body }
    })
}

fn section(name: &str) -> Option<Section> {
    sections().find(|s| s.name == name)
}

fn command_signature(name: &str, faults: bool) -> Option<String> {
    if name == "search" { Some(SEARCH_SIGNATURE.to_owned()) } else { find(name, faults).map(signature) }
}

/// What `mork ctl <command> --help` prints: its signature from the catalog, then its reference.
pub fn command_help(name: &str) -> Option<String> {
    let line = command_signature(name, false)?;
    let body = section(name).map_or("", |s| s.body);
    Some(format!("{line}\n\n{body}\n"))
}

/// A command's exact syntax and an excerpt from its own help.
#[derive(Serialize)]
pub struct CommandMatch {
    pub signature: String,
    pub snippet: String,
}

/// Commands matching every query word, case-insensitively, in reference order.
pub fn search(query: &str) -> Vec<CommandMatch> {
    let words: Vec<_> = query.split_whitespace().map(fold).collect();
    if words.is_empty() {
        return Vec::new();
    }
    sections().filter_map(|entry| entry.matching_command(&words)).collect()
}

impl Section {
    fn matching_command(&self, words: &[String]) -> Option<CommandMatch> {
        let signature = command_signature(self.name, false)?;
        let opening = self.body.lines().next().unwrap_or_default();
        let lead = fold(&format!("{signature}\n{opening}"));
        let remaining: Vec<_> = words.iter().filter(|word| !lead.contains(word.as_str())).cloned().collect();
        let snippet = if remaining.is_empty() {
            excerpt(self.summary, &[])
        } else {
            let body = fold(self.body);
            if !remaining.iter().all(|word| body.contains(word)) {
                return None;
            }
            excerpt(self.body, &remaining)
        };
        Some(CommandMatch { signature, snippet })
    }
}

/// What `mork test --help` prints: the reference's `mork test` sections, each form with its example.
pub fn test_help() -> String {
    let names = ["mork test", "mork test --run", "mork test --file", "mork test --checks", "mork test --walls"];
    let parts: Vec<String> =
        names.iter().filter_map(|n| section(n)).map(|s| format!("{}\n\n{}", s.name, s.body)).collect();
    format!("{}\n", parts.join("\n\n"))
}

// The API index, on demand (`mork manual index`): each command's signature and what it is
// for, then the helpers'. No start gives it.
fn index() -> String {
    let mut out = String::from(
        "# The API index\n\nEvery `mork ctl` command, its signature as the engine's catalog defines it: its required fields in order, then its optional ones as `--<field>`; boolean flags take no value; list fields accept comma-separated items or a JSON array of strings in one quoted argument, preserving commas inside items. `mork ctl <command> --help` gives one command's whole reference, `mork manual reference` all of it, and `mork manual` lists every manual.\n\n",
    );
    let commands = CATALOG.iter().filter(|c| !c.faults_only).map(|c| (c.name, signature(c)));
    for (name, line) in commands.chain([("search", SEARCH_SIGNATURE.to_owned())]) {
        let summary = section(name).map_or("", |s| s.summary);
        let _ = writeln!(out, "- `{line}`: {}", one_line(summary));
    }
    out.push_str("\nHelpers:\n");
    for s in sections().filter(|s| s.name.starts_with("mork ")) {
        let _ = writeln!(out, "- `{}`: {}", s.name, one_line(s.summary));
    }
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// The whole reference: its text, each command's signature under its heading.
fn reference() -> String {
    let mut out = String::new();
    for line in REFERENCE.lines() {
        out.push_str(line);
        out.push('\n');
        if let Some(syntax) = line.strip_prefix("### ").and_then(|n| command_signature(n.trim(), true)) {
            let _ = write!(out, "\n`{syntax}`\n");
        }
    }
    out
}

/// A manual by the name `mork manual` takes: `mork`, `reference`, `index`, `contract`, or
/// a role's.
pub fn manual(name: &str) -> Option<String> {
    match name {
        "mork" => Some(MORK.to_owned()),
        "reference" => Some(reference()),
        "index" => Some(index()),
        "contract" => Some(format!("# The reply contract\n\n{CONTRACT}\n")),
        n => ROLES.iter().find(|r| r.as_str() == n).map(|r| role_file(*r).to_owned()),
    }
}

/// The names `mork manual` takes, in order.
pub fn names() -> Vec<&'static str> {
    let mut out = vec!["mork", "index", "reference", "contract"];
    out.extend(ROLES.iter().map(|r| r.as_str()));
    out
}

/// The material a session starts with, by name (`mork`, `contract`, `role:<role>`):
/// what a start compares with the build that ran before it.
pub fn material() -> BTreeMap<String, String> {
    let mut out = BTreeMap::from([("mork".to_owned(), MORK.to_owned()), ("contract".to_owned(), CONTRACT.to_owned())]);
    for r in ROLES {
        out.insert(format!("role:{}", r.as_str()), role_file(r).to_owned());
    }
    out
}

/// Each piece of [`material`]'s digest.
pub fn digests() -> BTreeMap<String, String> {
    material().into_iter().map(|(k, v)| (k, mork_types::hash::short(&v))).collect()
}
