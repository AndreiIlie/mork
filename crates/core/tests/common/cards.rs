use factory_core::decide::{Decision, Event, Intent};
use factory_types::CardId;
use factory_types::action::ActionWhat;
use factory_types::card::Stage;
use factory_types::work::{PrSeen, PrState, Purpose};
use serde_json::{Value, json};

use super::{Sim, reply};

impl Sim {
    /// A full card started by Andrei, cloned, its brief the shortest way: a recap, a challenge with no objection, his
    /// approve.
    pub fn through_brief(&mut self, card: &str, project: &str) {
        self.board(card, project);
        self.andrei(json!({"start": {"card": card}})).unwrap();
        self.settle_actions();
        let id = self.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == card))[0].clone();
        self.end_job(&id, json!({}));
        self.settle_actions();
        assert_eq!(self.stage(card), Stage::Brief);
        let mut recap = reply("brief-ready", "The plan");
        recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
        self.turn(&format!("{card}/planner"), recap);
        self.turn(&format!("{card}/foreman"), reply("working", "No objection"));
        self.andrei(json!({"approve": {"card": card}})).unwrap();
        self.settle_actions();
        assert_eq!(self.stage(card), Stage::Build);
    }

    /// The card's foreman ends `built` on head `head`, forked from `fork`.
    pub fn built(&mut self, card: &str, head: &str, fork: &str) {
        self.turn(&format!("{card}/foreman"), reply("built", "Built"));
        self.take(card, head, fork, &["src/a.rs"]);
    }

    /// The fix requests a failed round made waits for its proofs, copied into the card's clone.
    pub fn proofs_copied(&mut self) {
        let copies: Vec<(CardId, u32)> = self
            .intents
            .iter()
            .filter_map(|i| match i {
                Intent::CopyProofs { review, round, .. } => Some((review.clone(), *round)),
                _ => None,
            })
            .collect();
        assert!(!copies.is_empty(), "no proofs to copy");
        self.intents.retain(|i| !matches!(i, Intent::CopyProofs { .. }));
        for (review, round) in copies {
            self.apply(Event::ProofsCopied { review, round, result: Ok(1) });
        }
        self.settle_actions();
    }

    /// The id of the review card verifying our card.
    pub fn review_card(&self, card: &str) -> String {
        format!("branch-{}-{card}", self.card(card).project)
    }

    /// The card's review round asked: its review card's clone made, its foreman owed a turn.
    pub fn round_asked(&mut self, card: &str) {
        let rc = self.review_card(card);
        let ids = self.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == rc));
        assert_eq!(ids.len(), 1, "no review clone of {rc}; jobs {:?}", self.pending_jobs());
        self.end_job(&ids[0], json!({}));
        self.settle_actions();
        assert!(self.owed(&format!("{rc}/foreman")), "{rc}'s foreman isn't asked for its round");
    }

    /// The review card's foreman ends its round, having verified and resolved the fixes of its findings numbered
    /// `fixed`: `review-pass`, or `review-fail` with `findings`.
    pub fn round_ends(&mut self, card: &str, fixed: &[u64], findings: &[Value]) -> Decision {
        let rc = self.review_card(card);
        let resolves: Vec<Value> = fixed.iter().map(|n| json!({"resolve": {"card": rc, "n": n}})).collect();
        let mut r = reply(if findings.is_empty() { "review-pass" } else { "review-fail" }, "The round");
        r["findings"] = json!(findings);
        let d = self.turn_sending(&format!("{rc}/foreman"), &resolves, r);
        self.settle_actions();
        d
    }

    /// The card's PR read by the poller on its pushed head, with Andrei's approval standing or not.
    pub fn pr_read(&mut self, card: &str, approved: bool) -> Decision {
        let c = self.card(card);
        let head = c.pushed.clone().unwrap();
        let seen = PrSeen {
            pr: c.pr.unwrap(),
            state: PrState::Open,
            head,
            approved,
            merge_parents: Vec::new(),
            merge_commit: None,
            dest: Some("master".into()),
        };
        self.apply(Event::PrRead { card: c.id, seen, before_merge: false })
    }

    /// Whether the card has entered the merge queue's landing or its PR's publication: any base move, push, PR opening
    /// or merge.
    pub fn landing_asked(&self, card: &str) -> bool {
        self.has_action(|w| match w {
            ActionWhat::MoveBase { card: c, .. }
            | ActionWhat::Push { card: c, .. }
            | ActionWhat::OpenPr { card: c, .. }
            | ActionWhat::Merge { card: c, .. } => c.as_str() == card,
            _ => false,
        })
    }
}
