//! What a reload brought (fac-75): the commits between the two builds, read from Mork's
//! repo, for the reload's line in main.

use std::path::Path;
use std::time::Duration;

use mork_core::maintenance::changelog::Commit;

use super::BuildError;
use crate::git;
use crate::paths::Home;

/// The commits `to` has and `from` hasn't, oldest first, and when the earliest of them was committed
/// (epoch ms): from Mork's mirror when it holds both, else the operator's checkout.
pub fn range(
    home: &Home,
    config: &mork_types::config::Config,
    from: &str,
    to: &str,
) -> Result<(Vec<Commit>, Option<i64>), BuildError> {
    let id = config.mork.project.as_ref().ok_or(BuildError::NoSelfBuild)?;
    let (checkout, _) = super::mork_repo(config)?;
    let mirror = home.mirrors().join(id.as_str());
    let holds = |repo: &Path| [from, to].iter().all(|c| git::resolve(home, repo, c).is_ok());
    let repo = if mirror.join("HEAD").exists() && holds(&mirror) { mirror } else { checkout };
    let span = format!("{from}..{to}");
    let args = ["log", "--topo-order", "--reverse", "--format=%H%x1f%P%x1f%s", &span, "--"];
    let out = git::git(home, &repo, &args, Duration::from_secs(20))?;
    let times = git::git(home, &repo, &["log", "--format=%ct", &span, "--"], Duration::from_secs(20)).ok();
    let at = times.and_then(|t| t.lines().filter_map(|l| l.trim().parse::<i64>().ok()).min()).map(|s| s * 1000);
    Ok((out.lines().filter_map(parse).collect(), at))
}

fn parse(line: &str) -> Option<Commit> {
    let mut parts = line.splitn(3, '\u{1f}');
    let sha = parts.next()?.to_owned();
    let parents = parts.next()?.split_whitespace().map(str::to_owned).collect();
    Some(Commit { sha, parents, subject: parts.next().unwrap_or_default().to_owned() })
}
