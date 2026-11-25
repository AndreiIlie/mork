//! `factory test --run` on the factory's own project (fac-73): inside a card's wall no inner wall can start and the
//! app's tests build only with its `faults`; a run there gets both, and the fakes on the first port range.

use factory_types::config::ProjectConfig;

const APP: &str = "factory-app";
const FAULTS: &str = "factory-app/faults";

/// The argv of a run lease of the factory's own project: `command` under `env` with the test settings, and the app's
/// `faults` added to a `cargo test` that builds the app and names no features.
pub fn argv(project: &ProjectConfig, command: &[String]) -> Vec<String> {
    if command.is_empty() {
        return Vec::new();
    }
    let mut argv = vec!["env".to_owned(), "FACTORY_UNWALLED=1".to_owned()];
    if let Some(p) = project.ports.iter().find(|p| p.from < p.to) {
        argv.push(format!("FACTORY_TEST_PORTS={}-{}", p.from, p.to));
    }
    let mut command = command.to_vec();
    if let Some(at) = cargo_test(&command).filter(|&at| builds_the_app(&command[at + 1..])) {
        command.splice(at + 1..at + 1, ["--features".to_owned(), FAULTS.to_owned()]);
    }
    argv.extend(command);
    argv
}

/// Builds one wall-test file's command, refusing paths, options and extra filters (fac-480).
pub fn walls_argv(project: &ProjectConfig, words: &[String]) -> Result<Vec<String>, String> {
    let production = words.first().is_some_and(|w| w == "--no-default-features");
    let words = &words[usize::from(production)..];
    let Some(file) = words.first() else {
        return Err("`factory test --walls` needs a plain test-file name".into());
    };
    if file.is_empty()
        || file.starts_with('-')
        || !file.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("`factory test --walls` needs a plain test-file name, without a path or flag".into());
    }
    if words.len() > 2
        || words.get(1).is_some_and(|w| {
            w.is_empty() || w.starts_with('-') || w.chars().any(|c| c.is_whitespace() || c.is_control())
        })
    {
        return Err("`factory test --walls` accepts at most one filter word that is no flag".into());
    }
    let mut argv = vec!["env".into(), "-u".into(), "FACTORY_UNWALLED".into()];
    if let Some(p) = project.ports.iter().find(|p| p.from < p.to) {
        argv.push(format!("FACTORY_TEST_PORTS={}-{}", p.from, p.to));
    }
    argv.extend(["cargo".into(), "test".into(), "-p".into(), APP.into(), "--test".into(), file.clone()]);
    if production {
        argv.push("--no-default-features".into());
    } else {
        argv.extend(["--features".into(), FAULTS.into()]);
    }
    argv.extend(words.get(1).cloned());
    Ok(argv)
}

// Where cargo's subcommand is, when it is `test` (or its alias `t`): past cargo's own flags.
fn cargo_test(command: &[String]) -> Option<usize> {
    if command.first()?.rsplit('/').next() != Some("cargo") {
        return None;
    }
    let at = command.iter().skip(1).position(|a| !a.starts_with('-') && !a.starts_with('+'))? + 1;
    matches!(command[at].as_str(), "test" | "t").then_some(at)
}

// Features named by hand are the runner's choice (`--no-default-features` asks for the production
// build). From the clone's root, a `cargo test` naming no package builds every member, the app's too.
fn builds_the_app(args: &[String]) -> bool {
    let (mut named, mut excluded, mut workspace, mut manifest) = (Vec::new(), false, false, false);
    let mut args = args.iter().map(String::as_str).take_while(|a| *a != "--");
    while let Some(a) = args.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ if a.len() > 2 && (a.starts_with("-p") || a.starts_with("-F")) => (&a[..2], Some(&a[2..])),
            _ => (a, None),
        };
        let mut value = || inline.or_else(|| args.next());
        match flag {
            "--features" | "-F" | "--all-features" | "--no-default-features" => return false,
            "-p" | "--package" => named.extend(value()),
            "--exclude" => excluded |= value() == Some(APP),
            "--manifest-path" => manifest = true,
            "--workspace" | "--all" => workspace = true,
            _ => {}
        }
    }
    if workspace || (named.is_empty() && !manifest) { !excluded } else { named.contains(&APP) }
}
