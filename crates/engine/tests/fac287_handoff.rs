// fac-287: a card's handoff is copied through no symlink; a handoff of links alone is no handoff.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::symlink;

use factory_engine::runtime::handoff::preserve;

fn names(files: &[factory_types::note::HandoffFile]) -> Vec<&str> {
    files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn fac287_a_handoff_is_copied_through_no_link() {
    let root = std::env::temp_dir().join(format!("factory-test-handoff-{}", std::process::id()));
    let (clone, to, secret) = (root.join("clone"), root.join("records/handoffs/hf-1-1"), root.join("secret"));
    let handoff = clone.join(".git/factory/handoff");
    std::fs::create_dir_all(handoff.join("notes")).unwrap();
    std::fs::create_dir_all(secret.join("deep")).unwrap();
    std::fs::write(secret.join("key"), "SECRET").unwrap();
    std::fs::write(handoff.join("README.md"), "read me").unwrap();
    std::fs::write(handoff.join("notes/next.md"), "next").unwrap();
    symlink(secret.join("key"), handoff.join("key")).unwrap();
    symlink(&secret, handoff.join("up")).unwrap();
    symlink(&secret, handoff.join("notes/deeper")).unwrap();
    let copied = preserve(&clone, &to).unwrap().unwrap();
    assert_eq!(names(&copied.files), ["README.md", "notes/next.md"]);
    assert_eq!(copied.skipped, ["key", "notes/deeper", "up"]);
    assert_eq!(std::fs::read_to_string(to.join("notes/next.md")).unwrap(), "next");
    for left in ["key", "up/key", "notes/deeper/key"] {
        assert!(!to.join(left).exists(), "{left} was copied");
    }
    std::fs::remove_dir_all(&to).unwrap();
    for file in ["README.md", "notes/next.md"] {
        std::fs::remove_file(handoff.join(file)).unwrap();
    }
    assert!(preserve(&clone, &to).unwrap().is_none(), "a handoff of links alone was kept");
    assert!(!to.exists());
    std::fs::remove_dir_all(&handoff).unwrap();
    symlink(&secret, &handoff).unwrap();
    assert!(preserve(&clone, &to).unwrap().is_none(), "a link for the handoff was followed");
    std::fs::remove_file(&handoff).unwrap();
    std::fs::remove_dir_all(clone.join(".git")).unwrap();
    std::fs::create_dir_all(secret.join("factory/handoff")).unwrap();
    std::fs::write(secret.join("factory/handoff/key"), "SECRET").unwrap();
    symlink(&secret, clone.join(".git")).unwrap();
    assert!(preserve(&clone, &to).unwrap().is_none(), "a link for .git was followed");
    assert!(!to.exists());
    std::fs::remove_dir_all(&root).unwrap();
}
