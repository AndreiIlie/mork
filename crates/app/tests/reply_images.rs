// fac-278, fac-654: a published image, a reply's or a say's, is a job's copy of a file beneath its seat's `io/`; links,
// others' files, `..`, non-regular files and `published/` are refused. A clicked one is staged from beneath its folder.
#![cfg(feature = "faults")]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{CODEX_FOREMAN, Rig, reply};
use mork_types::card::Mode;
use serde_json::{Value, json};
use std::path::Path;

const REFUSED: &str = "isn't a regular file of your io/";

fn attach(name: &str, setup: Vec<Value>, images: Value) -> Value {
    let mut steps = vec![json!({"do": "thread-started"})];
    steps.extend(setup);
    let mut r = reply("working", "Attached", json!([]));
    r["hand"]["images"] = images;
    steps.push(json!({"do": "codex-reply", "value": r}));
    json!({"fake": "codex", "agent": "ex-1/foreman", "phrase": format!("Attach {name}"), "steps": steps})
}

fn file(path: &str, text: &str) -> Value {
    json!({"do": "write-file", "path": path, "text": text})
}

fn sh(script: &str) -> Value {
    json!({"do": "sh", "script": script})
}

fn link(at: &str, target: &Path) -> Value {
    json!({"do": "symlink", "link": at, "target": target.display().to_string()})
}

// A case's name, what its turn does, the images it names, why each isn't published and its path's end.
type Case = (&'static str, Vec<Value>, Value, &'static str, &'static str);

fn escapes(secret: &Path, outside: &Path) -> Vec<Case> {
    let looped = "Too many levels of symbolic links";
    let theirs = "not in your io/ folder";
    vec![
        ("link", vec![link("{io}/link.png", secret)], json!(["{io}/link.png"]), looped, "/link.png"),
        ("dirlink", vec![link("{io}/dir", outside)], json!(["{io}/dir/x.png"]), looped, "/dir/x.png"),
        ("other", vec![], json!(["{jobs}/20200101-other/io/x.png"]), theirs, "/20200101-other/io/x.png"),
        ("seat", vec![], json!(["{seats}/20200101-other/x.png"]), theirs, "/io/20200101-other/x.png"),
        ("dots", vec![], json!(["{io}/../20200101-other/x.png"]), "not a plain path", "/../20200101-other/"),
        ("missing", vec![], json!(["{io}/missing.png"]), "No such file", "/missing.png"),
        ("dir", vec![sh("mkdir {io}/dir.png")], json!(["{io}/dir.png"]), "not a regular file", "/dir.png"),
        ("fifo", vec![sh("mkfifo {io}/pipe.png")], json!(["{io}/pipe.png"]), "not a regular file", "/pipe.png"),
        (
            "forged",
            vec![sh("mkdir -p {job}/published && printf SECRET > {job}/published/1-f.png")],
            json!(["{job}/published/1-f.png"]),
            theirs,
            "/published/1-f.png",
        ),
    ]
}

// The correction first, as its phrase is in every refused turn's prompt, reading what the good turn left in its seat's
// folder; then the good turn, the cases, and any other turn, which overwrites the good turn's shot there.
fn scenario(cases: &[Case]) -> Value {
    let mut good = reply(
        "working",
        "Attached",
        json!([{"to": "foreman", "kind": "request", "text": "Look", "images": ["{io}/two.png"]}]),
    );
    good["hand"]["images"] = json!(["{io}/shot"]);
    let mut runs = vec![
        json!({"fake": "codex", "agent": "ex-1/foreman", "phrase": REFUSED, "steps": [{"do": "thread-started"}, sh("cat {io}/two.png > {io}/kept"), {"do": "codex-reply", "value": reply("working", "Fixed", json!([]))}]}),
    ];
    runs.push(json!({"fake": "codex", "agent": "ex-1/foreman", "phrase": "Attach good", "steps": [
        {"do": "thread-started"}, file("{io}/shot", "SHOT"), file("{io}/two.png", "TWO"), {"do": "codex-reply", "value": good}]}));
    runs.extend(cases.iter().map(|(name, setup, images, ..)| attach(name, setup.clone(), images.clone())));
    runs.push(json!({"fake": "codex", "agent": "ex-1/foreman", "steps": [{"do": "thread-started"}, file("{io}/shot", "CHANGED"), {"do": "codex-reply", "value": reply("working", "Seen", json!([]))}]}));
    Value::Array(runs)
}

#[test]
fn fac278_reply_images_are_published_from_their_seats_io_only() {
    let mut rig = Rig::new();
    rig.write_config(&rig.config(CODEX_FOREMAN));
    let c = rig.card("ex-1", Mode::Full);
    rig.put_cards(vec![c]);
    let (secret, outside) = (rig.t.root.join("secret.png"), rig.t.root.join("outside"));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(&secret, "SECRET").unwrap();
    std::fs::write(outside.join("x.png"), "SECRET").unwrap();
    for other in [rig.fhome().join("jobs/20200101-other/io"), rig.fhome().join("io/20200101-other")] {
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("x.png"), "OTHER").unwrap();
    }
    let cases = escapes(&secret, &outside);
    rig.scenario(scenario(&cases));
    rig.start();
    rig.say("ex-1", "@foreman Attach good");
    rig.wait_turns("ex-1/foreman", 2);
    for (i, (name, ..)) in cases.iter().enumerate() {
        rig.say("ex-1", &format!("@foreman Attach {name}"));
        rig.wait_turns("ex-1/foreman", 4 + 2 * i);
    }
    let jobs = rig.jobs("ex-1/foreman");
    let published: Vec<String> = rig.state().messages.iter().flat_map(|m| m.images.clone()).collect();
    let first = jobs.iter().find(|j| j.stdin.contains("Attach good")).unwrap();
    let published_dir = rig.job_dir(first).join("published");
    let (shot, two) = (published_dir.join("1-shot"), published_dir.join("2-two.png"));
    let want = vec![shot.display().to_string(), two.display().to_string()];
    assert_eq!(published, want, "only the good turn's images are on any message, the forged one's not");
    assert_eq!(std::fs::read_to_string(&shot).unwrap(), "SHOT", "the published copy didn't change with its source");
    assert_eq!(std::fs::read_to_string(&two).unwrap(), "TWO");
    for (name, _, _, why, tail) in &cases {
        let said = |l: &str| l.contains(REFUSED) && l.contains(&format!("not published ({why}")) && l.contains(tail);
        assert!(
            jobs.iter().any(|j| j.stdin.lines().any(said)),
            "{name}: no correction said it wasn't published, {why}"
        );
    }
    let refused = jobs.iter().filter(|j| j.stdin.contains(REFUSED)).count();
    assert_eq!(refused, cases.len(), "each escape was refused, and corrected once");
    for j in &jobs {
        for f in std::fs::read_dir(rig.job_dir(j).join("published")).into_iter().flatten().flatten() {
            let (text, forged) = (std::fs::read_to_string(f.path()).unwrap(), f.file_name() == "1-f.png");
            assert!(text != "OTHER" && (text != "SECRET" || forged), "{} published {text}", f.path().display());
        }
    }
    kept_in_its_seat(&rig, &jobs, &two);
}

// Every turn, corrections too, had the one seat folder, where the good turn's files stayed; the next turn read the
// published copy into it, and overwriting the shot there left that copy as it was.
fn kept_in_its_seat(rig: &Rig, jobs: &[mork_types::state::Job], two: &Path) {
    let seat = jobs[0].spec.env.get("MORK_IO").cloned().unwrap();
    assert!(seat.starts_with(&rig.fhome().join("io/").display().to_string()), "{seat} is a seat's folder");
    for j in jobs {
        assert_eq!(j.spec.env.get("MORK_IO"), Some(&seat), "{} had its seat's folder", j.id);
    }
    let seat = Path::new(&seat);
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert_eq!(read(&seat.join("kept")), "TWO", "a correction read what the good turn left");
    assert_eq!(read(&seat.join("shot")), "CHANGED");
    let next = jobs.iter().find(|j| j.io_copies.values().any(|s| s == &two.display().to_string())).unwrap();
    let (copy, _) = next.io_copies.iter().find(|(_, s)| *s == &two.display().to_string()).unwrap();
    assert_eq!(copy, &format!("images/{}-1-2-two.png", next.id), "named by the job bringing it");
    assert_eq!(read(&seat.join(copy)), "TWO");
}

// A foreman's `say`s to itself (fac-654): a refused one leaves no copy, and the sent one's images are copied as it is
// sent; the turn it wakes fails once, overwriting the files it was given, and its retry is given that copy again.
fn says() -> Value {
    let say = |to: &str, images: &str, text: &str| {
        let args = ["say", "ex-1", "--to", to, "--kind", "request", "--images", images, text];
        json!({"do": "ctl", "args": args})
    };
    let send = json!([{"do": "thread-started"}, file("{io}/a.png", "ORIG"), file("{io}/b.png", "B"),
        say("researcher-99", "{io}/a.png", "Lost"), say("foreman", "{io}/b.png,{io}/missing.png", "Partial"),
        say("foreman", "{io}/a.png,{io}/a.png", "Look at it"), file("{io}/a.png", "CHANGED"),
        {"do": "codex-reply", "value": reply("working", "Sent", json!([]))}]);
    let once = "test -e {io}/tried && exit 0; touch {io}/tried; cat {io}/images/* > {io}/seen; \
        for f in {io}/images/*; do printf CORRUPT > \"$f\"; done; printf AGAIN > {io}/a.png; kill -KILL $PPID";
    let done = json!({"do": "codex-reply", "value": reply("working", "Seen", json!([]))});
    let seen = json!([{"do": "thread-started"}, sh(once), done]);
    json!([
        {"fake": "codex", "agent": "ex-1/foreman", "phrase": "Send it", "steps": send},
        {"fake": "codex", "agent": "ex-1/foreman", "steps": seen},
    ])
}

#[test]
fn fac654_a_said_image_is_copied_as_it_is_sent() {
    let mut rig = Rig::new();
    rig.write_config(&rig.config(CODEX_FOREMAN));
    let c = rig.card("ex-1", Mode::Full);
    rig.put_cards(vec![c]);
    rig.scenario(says());
    rig.start();
    rig.say("ex-1", "@foreman Send it");
    rig.wait_turns("ex-1/foreman", 2);
    rig.say("ex-1", "@foreman Go again");
    rig.wait_turns("ex-1/foreman", 3);
    let jobs = rig.jobs("ex-1/foreman");
    let sent = jobs.iter().find(|j| j.stdin.contains("Send it")).unwrap();
    let published = rig.job_dir(sent).join("published");
    let names: Vec<_> = std::fs::read_dir(&published).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, ["say-1-a.png"], "a refused say's copies were removed, an image named twice copied once");
    let copy = published.join("say-1-a.png").display().to_string();
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert_eq!(read(Path::new(&copy)), "ORIG", "the copy is the file as it was sent");
    let said: Vec<_> = rig.state().messages.into_iter().filter(|m| !m.images.is_empty()).collect();
    let said: Vec<_> = said.iter().map(|m| (m.text.as_str(), m.images.clone())).collect();
    assert_eq!(said, [("Look at it", vec![copy.clone(), copy.clone()])], "only the sent say is posted, with its copy");
    let ctl: Vec<Value> = rig.out_lines(sent).into_iter().filter(|l| l["type"] == "fake-ctl").collect();
    let codes: Vec<_> = ctl.iter().map(|l| l["code"].as_i64()).collect();
    assert!(codes[..2].iter().all(|c| *c != Some(0)) && codes[2] == Some(0), "{ctl:?}");
    let missing = ctl[1]["out"].as_str().unwrap_or_default();
    assert!(missing.contains("missing.png can't be sent (No such file"), "{ctl:?}");
    given_the_copy(&jobs[1..], &copy);
}

// The woken turn and its retry were each given the say's copy, the retry again after its first try overwrote them.
fn given_the_copy(turns: &[mork_types::state::Job], copy: &str) {
    let seat = Path::new(turns[0].spec.env.get("MORK_IO").unwrap());
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let woken = &turns[0].io_copies;
    assert!(!woken.is_empty() && woken.values().all(|s| s == copy), "{woken:?}");
    assert_eq!(
        read(&seat.join("seen")),
        "ORIG".repeat(woken.len()),
        "the woken turn was given the file as it was sent"
    );
    assert_eq!(read(&seat.join("a.png")), "AGAIN", "the sender's own file changed twice since");
    let retry = &turns[1];
    assert!(!retry.io_copies.is_empty() && retry.io_copies.values().all(|s| s == copy), "{:?}", retry.io_copies);
    for rel in retry.io_copies.keys() {
        assert_eq!(read(&seat.join(rel)), "ORIG", "{rel}: the retry was given the copy, not what its first try left");
    }
}

#[test]
fn fac278_a_clicked_image_is_staged_from_beneath_its_folder_only() {
    let t = mork_fakes::TestHome::new();
    let home = mork_engine::paths::Home::new(t.mork_home());
    let pasted = mork_engine::clipboard::save(&home, 1, 1, &[0, 0, 0, 255]).unwrap();
    let png = std::fs::read(&pasted).unwrap();
    let (job, seat, outside) = (home.jobs().join("20200101-j"), home.seat_io("20200101-s"), t.root.join("outside"));
    for d in [job.join("io/folder.png"), job.join("published"), job.join("other"), seat.clone(), outside.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    for f in [job.join("published/1-shot"), job.join("io/pic.png"), job.join("other/x.png"), outside.join("x.png")] {
        std::fs::write(f, &png).unwrap();
    }
    std::fs::write(seat.join("seat.png"), &png).unwrap();
    std::os::unix::fs::symlink(outside.join("x.png"), seat.join("link.png")).unwrap();
    std::fs::write(job.join("io/text.png"), "SHOT").unwrap();
    std::os::unix::fs::symlink(outside.join("x.png"), job.join("io/link.png")).unwrap();
    std::os::unix::fs::symlink(&outside, job.join("io/dir")).unwrap();
    assert!(std::process::Command::new("mkfifo").arg(job.join("io/fifo.png")).status().unwrap().success());
    let (viewer, at) = (t.root.join("viewer"), |rel: &str| job.join(rel).display().to_string());
    let stage = |path: &str| mork_engine::jobs::outputs::staged_image(&home, path, &viewer);
    let in_seat = |rel: &str| seat.join(rel).display().to_string();
    let good =
        [(at("published/1-shot"), "1-shot.png"), (at("io/pic.png"), "pic.png"), (in_seat("seat.png"), "seat.png")];
    for (path, name) in good {
        let copy = stage(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!((copy.file_name().unwrap().to_str().unwrap(), std::fs::read(&copy).unwrap()), (name, png.clone()));
    }
    stage(&pasted.display().to_string()).unwrap();
    let escapes = [
        (at("io/link.png"), "Too many levels of symbolic links"),
        (at("io/dir/x.png"), "Too many levels of symbolic links"),
        (at("io/folder.png"), "not a regular file"),
        (at("io/fifo.png"), "not a regular file"),
        (at("io/text.png"), "not a PNG or JPEG image"),
        (at("other/x.png"), "not in a job's io/ nor its published copies"),
        (at("io/../other/x.png"), "not a path inside its folder"),
        (in_seat("link.png"), "Too many levels of symbolic links"),
        (in_seat("../20200101-o/x.png"), "not a path inside its folder"),
        (outside.join("x.png").display().to_string(), "not a seat's, a job's or a pasted file"),
    ];
    for (path, why) in escapes {
        let e = stage(&path).map(|p| p.display().to_string()).expect_err(&path);
        assert!(e.to_string().contains(why), "{path}: {e}, not {why}");
    }
    let staged = std::fs::read_dir(&viewer).unwrap().count();
    assert_eq!(staged, 4, "only the four images were staged, each in a folder of its own");
}
