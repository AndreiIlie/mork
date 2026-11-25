// M2.1, R9: a symlink at any component, or a path leaving the folder, is refused.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;
use std::path::Path;

#[test]
fn m2_1_open_beneath_follows_no_link() {
    let root = std::env::temp_dir().join(format!("factory-sys-beneath-{}", std::process::id()));
    let io = root.join("io");
    std::fs::create_dir_all(io.join("sub")).unwrap();
    std::fs::write(root.join("secret"), "SECRET").unwrap();
    std::fs::write(io.join("sub/ok.png"), "PNG").unwrap();
    std::os::unix::fs::symlink(root.join("secret"), io.join("x.png")).unwrap();
    std::os::unix::fs::symlink(&root, io.join("up")).unwrap();
    let mut s = String::new();
    factory_sys::open_beneath(&io, Path::new("sub/ok.png")).unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "PNG");
    for bad in ["x.png", "up/secret", "../secret", "/etc/hostname"] {
        assert!(factory_sys::open_beneath(&io, Path::new(bad)).is_err(), "{bad} was opened");
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn r9_append_beneath_writes_through_no_link() {
    let root = std::env::temp_dir().join(format!("factory-sys-append-{}", std::process::id()));
    let clone = root.join("clone");
    std::fs::create_dir_all(clone.join(".git")).unwrap();
    std::fs::write(root.join("bashrc"), "OURS").unwrap();
    std::os::unix::fs::symlink(root.join("bashrc"), clone.join(".git/log")).unwrap();
    std::os::unix::fs::symlink(&root, clone.join("up")).unwrap();
    for bad in [".git/log", "up/bashrc", "../bashrc"] {
        assert!(factory_sys::append_beneath(&clone, Path::new(bad)).is_err(), "{bad} was opened");
    }
    assert_eq!(std::fs::read_to_string(root.join("bashrc")).unwrap(), "OURS");
    std::fs::remove_file(clone.join(".git/log")).unwrap();
    for line in ["one\n", "two\n"] {
        std::io::Write::write_all(
            &mut factory_sys::append_beneath(&clone, Path::new(".git/log")).unwrap(),
            line.as_bytes(),
        )
        .unwrap();
    }
    assert_eq!(std::fs::read_to_string(clone.join(".git/log")).unwrap(), "one\ntwo\n");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn r9_move_beneath_follows_no_link_and_replaces_nothing() {
    let root = std::env::temp_dir().join(format!("factory-sys-move-{}", std::process::id()));
    let (clone, aside, outside) = (root.join("clone"), root.join("aside"), root.join("outside"));
    for dir in [clone.join(".git/factory/proofs/sub"), aside.clone(), outside.join("proofs")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(clone.join(".git/factory/proofs/sub/proof"), "PROOF").unwrap();
    std::fs::write(outside.join("proofs/secret"), "SECRET").unwrap();
    std::os::unix::fs::symlink(&outside, clone.join("up")).unwrap();
    std::os::unix::fs::symlink(&outside, aside.join("up")).unwrap();
    std::os::unix::fs::symlink(outside.join("proofs"), clone.join("link")).unwrap();
    let proofs = Path::new(".git/factory/proofs");
    for (source, dest) in [("up/proofs", "kept"), (".git/factory/proofs", "up/kept"), ("link", "kept")] {
        assert!(factory_sys::move_dir_beneath(&clone, Path::new(source), &aside, Path::new(dest)).is_err());
    }
    std::fs::create_dir(aside.join("kept")).unwrap();
    assert!(factory_sys::move_dir_beneath(&clone, proofs, &aside, Path::new("kept")).is_err());
    assert_eq!(std::fs::read_dir(aside.join("kept")).unwrap().count(), 0);
    assert_eq!(std::fs::read_to_string(clone.join(proofs).join("sub/proof")).unwrap(), "PROOF");
    assert_eq!(std::fs::read_to_string(outside.join("proofs/secret")).unwrap(), "SECRET");
    assert!(!outside.join("kept").exists());
    std::fs::remove_dir(aside.join("kept")).unwrap();
    factory_sys::move_dir_beneath(&clone, proofs, &aside, Path::new("kept")).unwrap();
    assert!(!clone.join(proofs).exists());
    assert_eq!(std::fs::read_to_string(aside.join("kept/sub/proof")).unwrap(), "PROOF");
    std::fs::remove_dir_all(&root).unwrap();
}
