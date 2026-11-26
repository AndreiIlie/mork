use mork_types::CardId;
use mork_types::card::{Card, Mode};
use mork_types::review::{
    FindingState, FindingView, Raised, Review, Round, RoundView, TriageFindingView, TriageView, Verdict,
};
use serde_json::{Value, json};

use crate::decide::Decision;
use crate::reviews::gate::{id_of, subject_of};
use crate::state::State;

/// A finding's state: a drop outranks a decline, which outranks a plain resolve.
pub(crate) fn state_of(f: &Raised) -> FindingState {
    match (f.dropped.is_some(), f.declined, f.resolved) {
        (true, ..) => FindingState::Dropped,
        (false, true, _) => FindingState::Declined,
        (false, false, true) => FindingState::Resolved,
        (false, false, false) => FindingState::Open,
    }
}

fn to_view(f: &Raised) -> FindingView {
    FindingView {
        n: f.n,
        round: f.round,
        by: f.by.clone(),
        file: f.file.clone(),
        line: f.line,
        summary: f.summary.clone(),
        state: state_of(f),
        proof: f.proof.clone(),
    }
}

/// Parent card id when `card` is a live internal review card linked to a live parent card (fac-680).
pub(super) fn folded_parent(state: &State, card: &Card) -> Option<CardId> {
    if !card.stage.is_live() {
        return None;
    }
    let of = subject_of(card)?;
    state.cards.get(of).is_some_and(|p| p.stage.is_live()).then(|| of.clone())
}

/// Live folded internal review card of `parent`, if one exists (fac-680).
pub(super) fn folded_review<'a>(state: &'a State, parent: &Card) -> Option<&'a Card> {
    if parent.mode == Mode::Review || !parent.stage.is_live() {
        return None;
    }
    let reviews = |c: &&Card| c.stage.is_live() && subject_of(c) == Some(&parent.id);
    state.cards.get(&id_of(parent)).filter(reviews).or_else(|| state.cards.values().find(reviews))
}

// The card itself when it is a review card, else the one reviewing it, its branch card's id first.
pub(super) fn review_card<'a>(state: &'a State, card: &'a Card) -> Option<(&'a Card, &'a Review)> {
    if let Some(review) = card.review.as_deref() {
        return Some((card, review));
    }
    let reviews = |c: &&Card| subject_of(c) == Some(&card.id);
    let rc = state.cards.get(&id_of(card)).filter(reviews).or_else(|| state.cards.values().find(reviews))?;
    rc.review.as_deref().map(|review| (rc, review))
}

/// The current plan's latest non-superseded round number for `card` or its review card (fac-680).
pub(super) fn current_round(state: &State, card: &Card) -> Option<u32> {
    if let Some(t) = &card.triage {
        return Some(t.round);
    }
    let (_, review) = review_card(state, card)?;
    review.current().rev().find(|r| r.verdict != Some(Verdict::Superseded)).map(|r| r.n)
}

pub(super) fn round_findings<'a>(review: &'a Review, r: &Round) -> Vec<&'a Raised> {
    if !r.named.is_empty() {
        r.named.iter().filter_map(|&n| review.raised.iter().find(|f| f.n == n)).collect()
    } else {
        review.raised.iter().filter(|f| f.round == r.n).collect()
    }
}

pub(super) fn round_view(review: &Review, r: &Round) -> RoundView {
    let verdict = match r.verdict {
        Some(Verdict::Passed) => "passed",
        Some(Verdict::Failed) => "failed",
        Some(Verdict::Superseded) => "superseded",
        None => "under way",
    };
    let findings: Vec<FindingView> = round_findings(review, r).into_iter().map(to_view).collect();
    RoundView { n: r.n, commit: r.commit.short().to_owned(), verdict: verdict.to_owned(), findings }
}

/// Open failed-round triage data attached to the parent card's room view (fac-680).
pub(super) fn triage_for_card(state: &State, card: &Card) -> Option<TriageView> {
    let t = card.triage.as_ref()?;
    if !state.asks.get(&t.ask).is_some_and(|a| a.open) {
        return None;
    }
    let review = state.cards.get(&t.review)?.review.as_deref()?;
    let findings = t
        .items
        .iter()
        .filter_map(|item| {
            let f = review.raised.iter().find(|x| x.n == item.n)?;
            Some(TriageFindingView {
                n: f.n,
                by: f.by.clone(),
                file: f.file.clone(),
                line: f.line,
                summary: f.summary.clone(),
                proof: f.proof.clone(),
            })
        })
        .collect();
    Some(TriageView { ask: t.ask.clone(), review: t.review.clone(), round: t.round, stalemate: t.stalemate, findings })
}

/// `findings <card>`: its review card's raised findings, oldest first, each with its state.
pub(super) fn findings(state: &State, card: &str) -> Decision {
    let id = CardId::new(card.to_lowercase());
    let Some(asked) = state.cards.get(&id) else {
        return Decision::refused(format!("no card `{card}` in the state"));
    };
    let Some((rc, review)) = review_card(state, asked) else {
        return Decision::refused(format!("{id} has no review card"));
    };
    let list: Vec<Value> = review
        .raised
        .iter()
        .map(|f| {
            json!({"n": f.n, "round": f.round, "by": f.by, "file": f.file, "line": f.line,
                   "summary": f.summary, "proof": f.proof, "state": state_of(f)})
        })
        .collect();
    let of = subject_of(rc);
    Decision::ok(json!({"findings": {"card": rc.id.as_str(), "of": of, "findings": list}}))
}

#[cfg(test)]
mod tests {
    use mork_types::card::{Card, Mode};
    use mork_types::review::{Raised, Review, Round, Verdict};
    use mork_types::{ActionId, CardId, Sha};
    use serde_json::{Value, json};

    use crate::state::State;

    fn raised(n: u32, round: u32, resolved: bool, declined: bool, dropped: bool) -> Raised {
        Raised {
            n,
            round,
            by: "reviewer-1".into(),
            file: "a.rs".into(),
            line: 3,
            summary: format!("finding {n}"),
            proof: "proof".into(),
            action: None,
            comment: None,
            resolved,
            declined,
            withdrawn: false,
            thread: None,
            thread_error: None,
            answer: None,
            dropped: dropped.then(|| ActionId::new("act-1")),
        }
    }

    fn round(n: u32, verdict: Verdict, named: Vec<u32>) -> Round {
        Round {
            test_changes: Vec::new(),
            n,
            head: None,
            commit: Sha::new("abcdef123456"),
            base: Sha::new("111111111111"),
            files: Default::default(),
            carried: Default::default(),
            asked: Some(1),
            verdict: Some(verdict),
            proofs: None,
            submitted: None,
            named,
            reviewers: Default::default(),
        }
    }

    fn state() -> State {
        use mork_types::card::Stage;

        let mut state = State::default();
        let ours = Card {
            id: CardId::new("fac-1"),
            project: "mork".into(),
            mode: Mode::Full,
            stage: Stage::Merging,
            ..Card::default()
        };
        let raised = vec![
            raised(1, 1, false, false, false),
            raised(2, 1, true, false, false),
            raised(3, 1, true, true, false),
            raised(4, 2, true, false, true),
        ];
        let rounds = vec![round(1, Verdict::Failed, vec![1, 2, 3]), round(2, Verdict::Failed, vec![1, 4])];
        let review = Review { of: Some(ours.id.clone()), raised, rounds, ..Review::default() };
        let rc = Card {
            id: CardId::new("branch-mork-fac-1"),
            project: "mork".into(),
            mode: Mode::Review,
            review: Some(Box::new(review)),
            ..Card::default()
        };
        let lone = Card { id: CardId::new("fac-2"), project: "mork".into(), mode: Mode::Full, ..Card::default() };
        for c in [ours, rc, lone] {
            state.cards.insert(c.id.clone(), c);
        }
        state
    }

    fn answer(card: &str) -> Result<Value, String> {
        let d = super::findings(&state(), card);
        d.answer.unwrap_or(Ok(Value::Null)).map_err(|e| e.message)
    }

    #[test]
    fn a_review_card_and_its_reviewed_card_answer_the_same_findings() {
        let by_review = answer("branch-mork-fac-1");
        assert_eq!(by_review, answer("FAC-1"));
        let v = by_review.unwrap_or_default();
        assert_eq!(v.pointer("/findings/card"), Some(&json!("branch-mork-fac-1")));
        assert_eq!(v.pointer("/findings/of"), Some(&json!("fac-1")));
        let states: Vec<&Value> = (0..4).filter_map(|i| v.pointer(&format!("/findings/findings/{i}/state"))).collect();
        assert_eq!(states, [&json!("open"), &json!("resolved"), &json!("declined"), &json!("dropped")]);
        assert_eq!(v.pointer("/findings/findings/0/summary"), Some(&json!("finding 1")));
    }

    #[test]
    fn an_unknown_card_or_one_with_no_review_is_refused() {
        assert_eq!(answer("fac-9"), Err("no card `fac-9` in the state".into()));
        assert_eq!(answer("fac-2"), Err("fac-2 has no review card".into()));
    }

    fn round_stages(
        pv: &mork_types::view::PlaceView,
    ) -> Vec<(mork_types::view::OverviewStageKind, &mork_types::review::RoundView)> {
        pv.overview
            .as_ref()
            .map(|o| o.stages.iter().filter_map(|st| st.round.as_ref().map(|r| (st.kind, r))).collect())
            .unwrap_or_default()
    }

    #[test]
    fn place_views_carry_review_rounds_newest_first_with_named_findings() {
        use mork_types::config::Config;
        use mork_types::review::FindingState;
        use mork_types::view::OverviewStageKind;

        let s = state();
        let cfg: Config = serde_json::from_value(json!({
            "mork": {"project": "mork"},
            "git": {"author": "Test <t@example.invalid>"},
            "projects": {}
        }))
        .unwrap();
        let outside = crate::views::outside::Outside::default();
        let ctx = crate::decide::Ctx { now: 0, config: &cfg, faults: false };

        let build = crate::views::place::place(&s, "fac-1", None, &outside, ctx);
        let spec = crate::views::place::place(&s, "fac-1/spec", None, &outside, ctx);
        let review = crate::views::place::place(&s, "branch-mork-fac-1", None, &outside, ctx);
        let lone = crate::views::place::place(&s, "fac-2", None, &outside, ctx);

        assert!(spec.overview.is_none());
        let build_rounds = round_stages(&build);
        let review_rounds = round_stages(&review);
        assert_eq!(build_rounds.len(), 2);
        assert_eq!(build_rounds.iter().map(|(k, _)| *k).collect::<Vec<_>>(), [OverviewStageKind::Verify; 2]);
        assert_eq!(review_rounds.iter().map(|(k, _)| *k).collect::<Vec<_>>(), [OverviewStageKind::Review; 2]);
        assert_eq!(
            build_rounds.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            review_rounds.iter().map(|(_, r)| *r).collect::<Vec<_>>()
        );

        let (r1, r2) = (build_rounds[0].1, build_rounds[1].1);
        let r1_states: Vec<FindingState> = r1.findings.iter().map(|f| f.state).collect();
        assert_eq!((r1.n, r1_states), (1, vec![FindingState::Open, FindingState::Resolved, FindingState::Declined]));
        let r2_ns: Vec<u32> = r2.findings.iter().map(|f| f.n).collect();
        assert_eq!((r2.n, r2_ns, r2.findings[1].state), (2, vec![1, 4], FindingState::Dropped));
        assert!(r1.findings.iter().chain(&r2.findings).all(|f| f.proof == "proof"));
        assert!(round_stages(&lone).is_empty());
    }
}
