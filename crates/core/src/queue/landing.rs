//! Landing (4.10, 4.11): the front published, the local base moved, the merge checked, an alarm raised, and a card
//! landed; its PR is `pr`'s.

use factory_types::action::{Action, ActionStatus, ActionWhat};
use factory_types::ask::{AskKind, Color};
use factory_types::card::{Candidate, Card, Landing, Mode, Stage};
use factory_types::work::{PrSeen, Purpose};
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

use crate::decide::{Ctx, Intent};
use crate::messages::asks::{self, Raise};
use crate::post::Poster;

fn base(card: &Card, ctx: Ctx<'_>) -> String {
    ctx.config.projects.get(&card.project).map_or("master".into(), |p| p.base.clone())
}

/// Verified (fac-375): at the merge queue's front the local base moves to its candidate, or on a PR repository the
/// candidate is pushed, the head recorded before, and its PR opened, the turns of the card held until it lands.
pub fn publish(p: &mut Poster<'_>, mut c: Card, k: Candidate, ctx: Ctx<'_>) {
    let (Some(commit), Some(tree)) = (k.commit, k.tree) else { return };
    let id = c.id.clone();
    let bitbucket = ctx.config.projects.get(&c.project).is_some_and(|x| x.bitbucket.is_some());
    let (tree, base, commit_at) = (Some(tree), k.on.clone(), Some(commit.clone()));
    if bitbucket {
        c.landing = Some(Landing::Published { tree, base, commit: commit_at, verified: true });
        // The head it replaces stays known too: the poll may read it until this push lands.
        if !c.verified_pushes.contains(&commit) {
            c.verified_pushes.push(commit.clone());
        }
        if c.pr.is_some() && c.pushed.as_ref() == Some(&commit) {
            super::pr::pushed(p, c, &commit);
            return;
        }
        let branch = c.branch.clone().unwrap_or_else(|| id.as_str().to_owned());
        p.put_card(c);
        p.act(ActionWhat::Push { card: id, branch, commit });
        return;
    }
    c.landing = Some(Landing::Merge { tree, base, commit: commit_at });
    c.base_tip = Some(k.on);
    super::landing::move_base(p, c, commit, ctx);
}

/// A push its name refused, proven unpushed, of a card still publishing that commit with nothing on that branch
/// published: dropped, and pushed on a name the push accepts (fac-394). Else given back.
pub fn repushed(p: &mut Poster<'_>, mut a: Action, words: &str, ctx: Ctx<'_>) -> Option<Action> {
    let ActionWhat::Push { card, branch, commit } = a.what.clone() else { return Some(a) };
    let refused = |b: &str| crate::board::tickets::refused(&ctx.config.internals.words, b).is_some();
    let proven = a.unwritten == Some(true) && !a.cancelled;
    let c = p.card(&card).filter(|c| proven && super::pr::published(c, &commit) && refused(&branch));
    let Some(mut c) = c else { return Some(a) };
    let before = a.clone();
    a.status = ActionStatus::Dropped;
    a.words = Some(words.to_owned());
    a.done_ms = Some(p.now);
    let id = a.id.clone();
    p.put_action(a);
    if c.branch.as_deref() == Some(branch.as_str()) {
        crate::lifecycle::branches::rebuild(p, &mut c, ctx);
    }
    let safe = c.branch.clone().filter(|b| !refused(b));
    let Some(to) = safe.filter(|_| crate::lifecycle::branches::unpublished(p, &c, &branch)) else {
        return Some(before);
    };
    p.journal("action.dropped", Some(card.as_str()), json!({"id": id, "words": words}));
    p.put_card(c);
    p.act(ActionWhat::Push { card, branch: to, commit });
    None
}

/// The local base moved to the tested head, in Andrei's checkout (4.11.3).
pub fn move_base(p: &mut Poster<'_>, mut c: Card, target: Sha, ctx: Ctx<'_>) {
    c.stage = Stage::Merging;
    let from = c.base_tip.clone().or(c.fork.clone()).unwrap_or_else(|| target.clone());
    let (id, project) = (c.id.clone(), c.project.clone());
    p.put_card(c.clone());
    p.act(ActionWhat::MoveBase { card: id, project, base: base(&c, ctx), from, target });
}

/// The local base moved, or the merge was refused (4.11.3). It counts only for the card still
/// landing that head; one that left meanwhile is alarmed.
pub fn moved(p: &mut Poster<'_>, mut c: Card, target: &Sha, v: &Value, ctx: Ctx<'_>) {
    let base_moved = v.get("base_moved").and_then(Value::as_bool) == Some(true);
    let landing = matches!(c.landing, Some(Landing::Merge { .. }));
    if c.stage != Stage::Merging || !landing || super::order::commit_of(&c).as_ref() != Some(target) {
        if !base_moved {
            let words = format!("{}'s base moved to {} after it left the merge queue", c.id, target.short());
            alarm(p, c, &words);
        }
        return;
    }
    if !base_moved {
        let after = v.get("after").and_then(Value::as_str).map(Sha::new);
        if let Some(project) = ctx.config.projects.get(&c.project)
            && project.bitbucket.is_none()
            && let Some(remote) = &project.backup
        {
            p.act(ActionWhat::BackupPush {
                project: c.project.clone(),
                base: project.base.clone(),
                remote: remote.clone(),
                landed: target.clone(),
            });
        }
        let span = after.is_none().then(|| (None, target.clone()));
        landed_at(p, c, Some(target.clone()), after, span, false, ctx);
        return;
    }
    let Some(tip) = v.get("base_tip").and_then(Value::as_str).map(Sha::new) else { return };
    // Never landed on a pass for other code: incorporated and validated again (fac-10).
    let words = format!("{}'s base moved to {} since its run", c.project, tip.short());
    if let (true, Some(head)) = (crate::reviews::gate::entry(&c), c.head.clone()) {
        let text = format!("{words}: it goes through the merge train's steps again on it before it lands.");
        crate::reviews::cards::to_andrei(p, &c.id, &text);
        return super::entries::enter(p, c, tip, head, ctx);
    }
    c.stage = Stage::Merging;
    super::train::again(p, c, &tip, &words, ctx);
}

/// An alarm on a card, raised once; nothing more runs for it (4.11.6).
pub fn alarm(p: &mut Poster<'_>, c: Card, words: &str) {
    alarm_on(p, c, words, None);
}

/// An alarm tied to the action it holds (a merge whose check failed): `action.done` answers
/// it, once Andrei has looked (4.11.2, R12).
pub fn alarm_on(p: &mut Poster<'_>, mut c: Card, words: &str, action: Option<factory_types::ActionId>) {
    if c.alarm {
        return;
    }
    c.alarm = true;
    c.paused = true;
    super::candidates::drop_candidate(p, &mut c);
    c.landing = None;
    let id = c.id.clone();
    p.put_card(c);
    // Its landing gone, nothing waits on its merge (fac-200).
    asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
    // Nor starts what waited to land it, a merge approved behind its turn or its stop among it; a held
    // one, and the one the alarm holds, stay for Andrei (fac-98).
    let waiting = |a: &Action| a.status == ActionStatus::Pending && Some(&a.id) != action.as_ref();
    super::members::drop_landing_actions(p, &id, "its card is alarmed", waiting);
    let mut r = Raise::new(id.as_str(), AskKind::Alarm, Color::Red, words.to_owned());
    r.action = action;
    asks::raise(p, r);
}

/// An alarm acked (fac-157): the PR's head the poller last read is the factory's own push, and a
/// head that passed its gates goes to the back of its train; a cause that still holds alarms again.
pub fn alarm_closed(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    c.alarm = false;
    c.paused = false;
    let prs = p.state.world.open_prs.get(&c.project);
    if let Some(pr) = c.pr.and_then(|n| prs.and_then(|all| all.iter().find(|x| x.pr == n))) {
        // A stale poll must not replace the push whose completed action recovery would dedup (fac-204).
        if c.previous_push.as_ref() != Some(&pr.head) {
            c.pushed = Some(pr.head.clone());
        }
    }
    let id = c.id.clone();
    p.put_card(c.clone());
    p.journal("alarm.closed", Some(id.as_str()), json!({}));
    let gated = c.head.is_some() && c.gates.passed == c.head;
    if gated && matches!(c.stage, Stage::Ready | Stage::Merging) {
        super::members::enter(p, c, ctx);
    }
}

/// The merge call returned: the merge commit's tree and first parent checked (4.11.2.5); a
/// company PR's merged on Andrei's approval, which no tested tree or base binds (fac-98).
pub fn merged(p: &mut Poster<'_>, c: Card, v: &Value, ctx: Ctx<'_>) {
    if c.mode == Mode::Review {
        return crate::reviews::end::merged(p, c, v, ctx);
    }
    // Landed or closed meanwhile: nothing more (R7).
    if c.stage != Stage::Merging {
        return;
    }
    let by_hand = v.get("by_hand").is_some();
    let approved = crate::queue::approval::waiting(&c);
    if !by_hand && !approved && !matches!(c.landing, Some(Landing::Merge { .. })) {
        let words = format!("PR {} was merged after {} left the merge queue", c.pr.unwrap_or_default(), c.id);
        return alarm(p, c, &words);
    }
    if by_hand || approved || v.get("matches").and_then(Value::as_bool) == Some(true) {
        let tip = v.get("commit").and_then(Value::as_str).map(Sha::new);
        let mut c = c;
        // Merged by hand, nothing checked its tree: what was built on it is made again (fac-68).
        if by_hand || approved {
            c.landing = None;
        }
        landed_at(p, c, tip.clone(), tip, None, false, ctx);
        return;
    }
    let why = v.get("why").and_then(Value::as_str).unwrap_or("the merge commit isn't what was tested");
    let words = format!("the merge of {} needs a look: {why}", c.id);
    alarm(p, c, &words);
}

/// The merge commit's check (4.11.2.5): its first parent is the fetched base tip and its tree the tested one (with no
/// join, the head's, then a parent). Adds `matches` and `why` to the merge's result.
pub fn merge_check(what: &ActionWhat, v: Value) -> Value {
    // A colleague's PR merges into a destination that may have moved: its guard was the check (fac-95).
    let ActionWhat::Merge { head, tree, base, dest: None, .. } = what else { return v };
    if v.get("by_hand").is_some() || v.get("matches").is_some() {
        return v;
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let parents: Vec<String> = v
        .get("parents")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let want = tree.clone().or_else(|| s("head_tree"));
    let why = if let Some(u) = s("unreadable") {
        Some(u)
    } else if parents.first().map(String::as_str) != Some(base.as_str()) {
        Some(format!(
            "its first parent is {}, not the base tip {} that was fetched",
            parents.first().map_or("none", String::as_str),
            base.short()
        ))
    } else if tree.is_none() && !parents.iter().any(|x| x == head.as_str()) {
        Some("it doesn't have the tested head as a parent".to_owned())
    } else if want.is_none() || want != s("tree") {
        Some("its tree isn't the tree that was tested".to_owned())
    } else {
        None
    };
    let mut v = v;
    if let Some(o) = v.as_object_mut() {
        o.insert("matches".into(), Value::Bool(why.is_none()));
        o.insert("why".into(), why.map_or(Value::Null, Value::String));
    }
    v
}

// Its PR merged by hand, or the card landed: none of its landing actions is left to start, a merge
// approved behind its turn among them, nor held with its ask; one under way returns as before (fac-98).
pub(crate) fn none_left(p: &mut Poster<'_>, id: &CardId, words: &str) {
    let left = |a: &Action| matches!(a.status, ActionStatus::Pending | ActionStatus::Held);
    super::members::drop_landing_actions(p, id, words, left);
}

// A merge made outside the factory has no action that could supply its bounds later (fac-75).
pub(crate) fn merge_span(seen: &PrSeen, head: &str) -> Option<(Option<Sha>, Sha)> {
    let base = seen.merge_parents.first().filter(|b| seen.merge_parents.len() > 1 && b.0 != head).cloned();
    Some((base, seen.merge_commit.clone()?))
}

/// Finishes a landing and schedules a build for the factory's new tip. `span` can record a landed
/// commit with no known previous base. `keep` takes the head into the mirror first (fac-171).
pub fn landed_at(
    p: &mut Poster<'_>,
    mut c: Card,
    tip: Option<Sha>,
    new_tip: Option<Sha>,
    span: Option<(Option<Sha>, Sha)>,
    keep: bool,
    ctx: Ctx<'_>,
) {
    if let Some(commit) = new_tip.clone().filter(|_| c.project == ctx.config.factory.project) {
        p.intent(Intent::StartJob { purpose: Purpose::Build { commit } });
    }
    c.stage = Stage::Done;
    c.landing = None;
    crate::reviews::triage::withdraw(p, &mut c);
    let (id, project, run) = (c.id.clone(), c.project.clone(), c.run);
    p.put_card(c.clone());
    // A branch's card that landed it ends with it, its agents released, before its clone goes (fac-111).
    if crate::reviews::gate::entry(&c) {
        crate::reviews::end::finish(p, c.clone(), ctx);
    }
    none_left(p, &id, "its card landed");
    for a in asks::open_of_card(p, id.as_str()) {
        asks::close_one(p, &a.id);
    }
    // Each card the run carries lands with it (fac-563).
    if let Some(pc) = ctx.config.projects.get(&project) {
        for of in crate::lifecycle::carried::cards(&c) {
            crate::lifecycle::carried::status_write(p, &c, of, &pc.statuses.merged, true);
        }
    }
    if keep {
        p.intent(Intent::StartJob {
            purpose: Purpose::Bundle { card: id.clone(), base_in: None, recover: None, refresh: None },
        });
    } else {
        p.act(ActionWhat::RemoveClone { card: id.clone(), run, keep_proofs: false });
    }
    crate::agents::closing::card_ended(p, &id, false, ctx.config);
    let mut row = json!({"head": c.head});
    let (before, after) = span.map_or((None, new_tip), |(before, after)| (before, Some(after)));
    if let Some(before) = before {
        row["before"] = json!(before);
    }
    if let Some(after) = after {
        row["after"] = json!(after);
    }
    p.journal("card.landed", Some(id.as_str()), row);
    crate::reviews::end::end(p, &c, ctx);
    super::joins::early(p, &c, tip.clone().or(c.head.clone()), ctx);
    if let Some(tip) = &tip {
        super::refresh::seen(p, project.as_str(), tip);
    }
    crate::lifecycle::overlaps::clear(p, &id);
    super::train::maintain(p, &project, ctx);
}
