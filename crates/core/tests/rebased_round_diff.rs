// fac-628: later review rounds with a moved base direct reviewers to `refs/rounds/<n>-on-base`
// without the merged base, name any projection conflicts, and fall back when projection failed.

// why: test code: panicking on an unexpected answer fails the test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, sha};
use factory_types::state::MessageKind;
use factory_types::work::Purpose;
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn round_request(s: &Sim, card: &str) -> String {
    let rc = s.review_card(card);
    s.engine_to(&rc, "foreman")
        .into_iter()
        .rfind(|m| m.kind == MessageKind::Request)
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

fn finish_round_one_and_move_base(s: &mut Sim) {
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "bug", "proof": "proof"});
    s.round_ends(CARD, &[], &[finding]);
    s.proofs_copied();
    s.suite(CARD, 0);
    s.andrei(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    s.settle_actions();
    s.imported(CARD);
    s.tip = "base2";
    s.built(CARD, "h2", "base2");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.suite(CARD, 0);
}

fn end_review_clone(s: &mut Sim, result: Value) {
    let rc = s.review_card(CARD);
    let ids = s.open_jobs(|p| matches!(p, Purpose::Clone { card, .. } if card.as_str() == rc));
    assert_eq!(ids.len(), 1);
    s.end_job(&ids[0], result);
    s.settle_actions();
}

#[test]
fn moved_base_clean_projection_names_on_base_ref() {
    let mut s = Sim::new();
    finish_round_one_and_move_base(&mut s);
    end_review_clone(&mut s, json!({"rebased": {"status": "projected", "conflicted": false, "conflicts": []}}));

    let text = round_request(&s, CARD);
    let (b1, b2, h2) = (sha("base1"), sha("base2"), sha("h2"));
    assert!(text.contains(&format!("git diff refs/rounds/1-on-base {h2}")), "{text}");
    assert!(text.contains(&format!("git diff {b1} {b2}")), "{text}");
    assert!(!text.contains(&format!("git diff refs/rounds/1 {h2}")), "{text}");
    assert!(!text.contains("conflicted in:") && !text.contains("reported conflicts"), "{text}");
}

#[test]
fn moved_base_conflicted_projection_lists_conflicts() {
    let mut s = Sim::new();
    finish_round_one_and_move_base(&mut s);
    end_review_clone(
        &mut s,
        json!({"rebased": {"status": "projected", "conflicted": true, "conflicts": ["src/a.rs", "src/b.rs"]}}),
    );

    let text = round_request(&s, CARD);
    assert!(text.contains(&format!("git diff refs/rounds/1-on-base {}", sha("h2"))), "{text}");
    assert!(text.contains("conflicted in: src/a.rs, src/b.rs"), "{text}");
}

#[test]
fn moved_base_conflicted_projection_without_paths_warns() {
    let mut s = Sim::new();
    finish_round_one_and_move_base(&mut s);
    end_review_clone(&mut s, json!({"rebased": {"status": "projected", "conflicted": true, "conflicts": []}}));

    let text = round_request(&s, CARD);
    assert!(text.contains(&format!("git diff refs/rounds/1-on-base {}", sha("h2"))), "{text}");
    assert!(text.contains("reported conflicts with no listed file paths"), "{text}");
}

#[test]
fn moved_base_unmerged_candidate_uses_round_ref() {
    let mut s = Sim::new();
    finish_round_one_and_move_base(&mut s);
    end_review_clone(&mut s, json!({"rebased": {"status": "unmerged"}}));

    let text = round_request(&s, CARD);
    let (b1, b2, h2) = (sha("base1"), sha("base2"), sha("h2"));
    assert!(text.contains(&format!("Commit `{h2}` does not contain base `{b2}`")), "{text}");
    assert!(text.contains(&format!("git diff refs/rounds/1 {h2}")), "{text}");
    assert!(text.contains(&format!("git diff {b1} {b2}")), "{text}");
    assert!(!text.contains("-on-base"), "{text}");
}

#[test]
fn moved_base_missing_projection_falls_back() {
    let mut s = Sim::new();
    finish_round_one_and_move_base(&mut s);
    end_review_clone(&mut s, json!({}));

    let text = round_request(&s, CARD);
    let (b1, b2, h2) = (sha("base1"), sha("base2"), sha("h2"));
    assert!(text.contains("did not produce a ref"), "{text}");
    assert!(text.contains(&format!("git diff refs/rounds/1 {h2}")), "{text}");
    assert!(text.contains(&format!("git diff {b1} {b2}")), "{text}");
    assert!(text.contains(&format!("git diff {b2} {h2}")), "{text}");
    assert!(!text.contains("-on-base"), "{text}");
}

#[test]
fn moved_base_projection_survives_install() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.config.projects.get_mut("web").unwrap().install = Some("npm ci".into());
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    end_review_clone(&mut s, json!({}));
    let rc = s.review_card(CARD);
    let inst1 = s.open_jobs(|p| matches!(p, Purpose::Install { card, .. } if card.as_str() == rc));
    assert_eq!(inst1.len(), 1);
    s.end_job(&inst1[0], json!({}));
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "bug", "proof": "proof"});
    s.round_ends(CARD, &[], &[finding]);
    s.proofs_copied();
    s.suite(CARD, 0);
    s.andrei(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    s.settle_actions();
    s.imported(CARD);
    s.tip = "base2";
    s.built(CARD, "h2", "base2");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.suite(CARD, 0);

    end_review_clone(
        &mut s,
        json!({"rebased": {"status": "projected", "conflicted": true, "conflicts": ["src/a.rs"]}}),
    );
    let inst2 = s.open_jobs(|p| matches!(p, Purpose::Install { card, .. } if card.as_str() == rc));
    assert_eq!(inst2.len(), 1);
    s.end_job(&inst2[0], json!({}));
    s.settle_actions();

    let text = round_request(&s, CARD);
    assert!(text.contains(&format!("git diff refs/rounds/1-on-base {}", sha("h2"))), "{text}");
    assert!(text.contains("conflicted in: src/a.rs"), "{text}");
}
