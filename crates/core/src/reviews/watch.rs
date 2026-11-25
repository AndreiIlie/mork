//! Which PRs review themselves (fac-98, fac-111): every PR into a repository's base, its results authorizing no
//! merge; the poller's open PRs are verified on first sight or a new head, our own verified head never again (fac-375).

use factory_types::CardId;
use factory_types::ask::AskKind;
use factory_types::card::{Card, Mode};
use factory_types::config::ProjectConfig;
use factory_types::review::Review;
use factory_types::work::OpenPr;

use crate::decide::{Ctx, Decision, Intent};
use crate::messages::asks;
use crate::post::Poster;
use crate::state::{Change, State};

/// Whether a card's repository has PRs: on Bitbucket, where every one merges on Andrei's approval
/// alone (fac-111).
pub fn prs(c: &Card, ctx: Ctx<'_>) -> bool {
    ctx.config.projects.get(&c.project).is_some_and(|p| p.bitbucket.is_some())
}

/// Whether a project's PRs into `dest` are watched: on any repository with PRs, those into its
/// base (master), each reviewed by itself and merged on Andrei's approval (fac-111).
pub fn watched(p: &ProjectConfig, dest: &str) -> bool {
    p.bitbucket.is_some() && dest == p.base
}

// Whether a PR is reviewed automatically: base PRs, and explicitly tagged review requests.
fn reviews_itself(project: &str, pr: &OpenPr, ctx: Ctx<'_>) -> bool {
    ctx.config.projects.get(project).is_some_and(|p| {
        watched(p, &pr.dest) || (p.bitbucket.is_some() && pr.title.contains("@review-bot"))
    })
}

/// The poller read a project's open PRs: those the rules review start by themselves (fac-98); no ask
/// is raised for the rest, and a `new-pr` ask of the project still open from before is closed (fac-273).
pub fn open_prs(state: &State, project: &str, prs: &[OpenPr], ctx: Ctx<'_>) -> Decision {
    let mut p = Poster::for_ctx(state, ctx);
    let prefix = format!("(review {project} ");
    let obsolete: Vec<factory_types::AskId> = p
        .all_asks()
        .filter(|a| a.open && a.kind == AskKind::NewPr && a.text.contains(&prefix))
        .map(|a| a.id.clone())
        .collect();
    for a in obsolete {
        asks::close_one(&mut p, &a);
    }
    review_open(&mut p, project, prs, ctx);
    left(&mut p, project, prs);
    p.other.push(Change::SetOpenPrs { project: project.into(), prs: prs.to_vec() });
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

// A live PR review card whose PR left the open list: its PR read, each poll until it ends, says merged
// or declined; one asked to merge is read every poll already (fac-161).
fn left(p: &mut Poster<'_>, project: &str, prs: &[OpenPr]) {
    let asked = |c: &Card| c.review.as_ref().is_some_and(|r| r.merge);
    let gone: Vec<CardId> = p
        .state
        .cards
        .values()
        .filter(|c| c.project == project && super::end::ends_with_pr(c) && !asked(c))
        .filter(|c| !prs.iter().any(|x| c.pr == Some(x.pr)))
        .map(|c| c.id.clone())
        .collect();
    for card in gone {
        p.intent(Intent::ReadPr { card });
    }
}

// Enters detected PRs into the train on first sight or head movement, reusing their review cards.
// Persisted card heads prevent duplicate runs after restart (fac-98, fac-111).
fn review_open(p: &mut Poster<'_>, project: &str, prs: &[OpenPr], ctx: Ctx<'_>) {
    let mut fetch = false;
    for pr in prs {
        crate::queue::approval::close_stale(p, project, pr);
        if ours(p, project, pr, ctx) {
            continue;
        }
        let card = p.card(&super::cards::card_id(project, pr.pr));
        let auto = reviews_itself(project, pr, ctx) || published(p, project, pr);
        match card {
            Some(mut c) if c.stage.is_live() => {
                let follows = auto || c.review.as_ref().is_some_and(|r| !r.auto);
                if follows && crate::queue::entries::moves(&c, &pr.head) {
                    if let Some(r) = c.review.as_deref_mut() {
                        r.dest = Some(pr.dest.clone()).filter(|d| !d.is_empty());
                        super::cards::copy_context(r, pr);
                    }
                    crate::queue::entries::moved(p, c, &pr.head);
                    fetch = true;
                } else if follows && crate::queue::entries::sync_open_head(&mut c, &pr.head) {
                    if let Some(r) = c.review.as_deref_mut() {
                        super::cards::copy_context(r, pr);
                    }
                    p.put_card(c);
                } else if let Some(r) =
                    c.review.as_deref_mut().filter(|r| r.tickets != pr.tickets || r.description != pr.description)
                {
                    // Context changes between heads too: a round yet to be asked reads the latest poll's.
                    super::cards::copy_context(r, pr);
                    p.put_card(c);
                }
            }
            Some(c) if !auto || crate::queue::entries::has(&c, &pr.head) => {}
            None if !auto => {}
            _ => {
                let dest = Some(pr.dest.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| crate::queue::approval::MASTER.to_owned());
                let review = Review { auto: true, dest: Some(dest), ..Review::default() };
                super::cards::start(p, project, pr, review);
                fetch = true;
            }
        }
    }
    if fetch {
        p.fetch(project);
    }
}

// Whether a PR is our live card's publication at a head its verification passed on the base it merges into (fac-375):
// no second review of it starts. Its PR by number, or before the number is known, by its branch; any other is reviewed.
fn ours(p: &Poster<'_>, project: &str, pr: &OpenPr, ctx: Ctx<'_>) -> bool {
    let Some(base) = ctx.config.projects.get(project).map(|c| c.base.as_str()) else { return false };
    let mine = |c: &Card| c.project == project && c.mode != Mode::Review && c.stage.is_live();
    let published = |c: &Card| match c.pr {
        Some(n) => n == pr.pr,
        None => c.branch.clone().unwrap_or_else(|| c.id.as_str().to_owned()) == pr.branch,
    };
    pr.dest == base && p.card_where(|c| mine(c) && published(c) && c.verified_pushes.contains(&pr.head)).is_some()
}

// Whether a PR is our live card's, published by an older build: its own entry reviews it (fac-111).
fn published(p: &Poster<'_>, project: &str, pr: &OpenPr) -> bool {
    crate::queue::approval::owner(p.state, project, pr.pr).is_some()
}
