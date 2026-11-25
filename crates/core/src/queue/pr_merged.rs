//! A full card whose PR is read as merged on Bitbucket, by the train or by hand, whatever its head
//! (fac-171): it lands at the merge commit, its head kept first, and never comes back to its train.

use factory_types::CardId;
use factory_types::ask::AskKind;
use factory_types::card::{Card, Landing};
use factory_types::work::PrSeen;
use serde_json::Value;

use crate::decide::Ctx;
use crate::messages::asks;
use crate::post::Poster;

/// Its PR read as merged, `head` what the card landing it pushed: it lands once the mirror holds
/// the merge commit, fetched first.
pub fn read(p: &mut Poster<'_>, mut c: Card, seen: &PrSeen, head: &str, ctx: Ctx<'_>) {
    if !seen.merge_parents.is_empty() {
        return land(p, c, seen, head, ctx);
    }
    // The merge commit isn't in the mirror yet: a fetch, guarded (2.5, M4.24), then the PR read again.
    // Whatever landing the card was at (a fetch, a join under way), it is merged by hand now (M5.15).
    if seen.merge_commit.is_none() || c.landing == Some(Landing::HandMerged) {
        return;
    }
    let (id, project) = (c.id.clone(), c.project.clone());
    crate::reviews::triage::withdraw(p, &mut c);
    c.landing = Some(Landing::HandMerged);
    p.put_card(c);
    // Nothing merges it any more, nor waits on its merge: his approval among it (fac-98).
    super::landing::none_left(p, &id, "its PR was merged outside the factory");
    asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
    p.fetch(project.as_str());
}

// Merged outside the factory: the merge commit is the base's new tip, a build's and the early
// joins' (1.8.4, 4.12.4; M4.21). Its alarm, which held it, closes with its other asks.
fn land(p: &mut Poster<'_>, mut c: Card, seen: &PrSeen, head: &str, ctx: Ctx<'_>) {
    // A turn under way could still commit: it ends, and what it committed is kept with the head.
    crate::lifecycle::stops::stop_jobs(p, &c.id, ctx);
    if c.pushed.as_ref() != Some(&seen.head) {
        foreign(p, &c, seen);
    }
    (c.alarm, c.paused) = (false, false);
    let tip = seen.merge_commit.clone().or_else(|| seen.merge_parents.first().cloned());
    let span = super::landing::merge_span(seen, head);
    super::landing::landed_at(p, c, tip, seen.merge_commit.clone(), span, true, ctx);
}

// Said once, as it lands: a note, which holds nothing.
fn foreign(p: &mut Poster<'_>, c: &Card, seen: &PrSeen) {
    let pushed = c.pushed.as_ref().map_or(String::new(), |x| format!(" (it pushed {})", x.short()));
    let at = seen.merge_commit.as_ref().or(seen.merge_parents.first()).map_or("", |m| m.short());
    let text = format!(
        "PR {} was merged with head {}, which the factory didn't push{pushed}: {} landed at its merge commit {at}.",
        seen.pr,
        seen.head.short(),
        c.id
    );
    crate::reviews::cards::to_andrei(p, &c.id, &text);
}

/// Its clone removed: the commits of its head that its PR's merge didn't take, which the engine
/// found in the mirror, are named to Andrei, kept there to carry on in a new card.
pub fn removed(p: &mut Poster<'_>, id: &CardId, v: &Value) {
    let commits: Vec<&str> = v
        .get("unmerged")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if commits.is_empty() {
        return;
    }
    let text = format!(
        "{id} landed, but these commits of its head aren't merged: {}. They are kept in the mirror as `refs/cards/{id}`, to carry on in a new card.",
        commits.join("; ")
    );
    crate::reviews::cards::to_andrei(p, id, &text);
}
