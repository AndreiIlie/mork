use factory_core::decide::Decision;
use factory_types::action::ActionWhat;
use factory_types::landing::Landing;
use factory_types::work::{CheckRun, Checkout, CloneStatus, FinalRun, Made, Purpose, Taken};
use serde_json::{Value, json};

use super::{Sim, contents, sha};

// Whether a purpose is card `id`'s.
fn of(p: &Purpose, id: &str) -> bool {
    p.card().is_some_and(|c| c.as_str() == id)
}

impl Sim {
    /// The card's bundle jobs answered with its clean clone at `head`, then the head, forked from `fork`, taken with
    /// `changed` files while the mirror holds the base's tip.
    pub fn take(&mut self, card: &str, head: &str, fork: &str, changed: &[&str]) {
        let status =
            CloneStatus { branch: card.into(), head: Some(sha(head)), clean: true, dirty: Vec::new(), bundle: true };
        let ids = self.open_jobs(|p| of(p, card) && matches!(p, Purpose::Bundle { .. }));
        assert!(!ids.is_empty(), "no pending bundle job; jobs {:?}", self.pending_jobs());
        for id in ids {
            self.end_job(&id, serde_json::to_value(&status).unwrap());
        }
        let a = self.action_of(|w| matches!(w, ActionWhat::Take { card: c, .. } if c.as_str() == card));
        let taken = Taken {
            head: sha(head),
            fork: sha(fork),
            changed: changed.iter().map(|s| (*s).to_owned()).collect(),
            lines: 10,
            rewrites_pushed: false,
            base_tip: sha(self.tip),
            contents: contents(head, changed),
        };
        self.returned(&a.id, Ok(serde_json::to_value(taken).unwrap()));
        self.settle_actions();
    }

    /// The checks gate's run on the card's current head ends with `exit` and the located `findings`.
    pub fn checks(&mut self, card: &str, exit: i32, findings: &[Value]) -> Decision {
        let head = self.card(card).head.unwrap();
        let report = json!({"status": exit, "findings": findings, "base": sha(self.tip), "head": head});
        let located = findings.iter().map(|f| Ok(f["file"].as_str().unwrap_or("").to_owned())).collect();
        let run = CheckRun {
            head,
            exit: Some(exit),
            report: Some(report),
            folder_commit: "c1".into(),
            folder_dirty: false,
            located,
            folder_moved: false,
        };
        let d = self.answer_job("check", |p| of(p, card) && matches!(p, Purpose::Check { .. }), json!(run));
        self.settle_actions();
        d
    }
}

impl Sim {
    /// The card's candidate made on a base its head contains: the head itself, its checks already passed.
    pub fn candidate(&mut self, card: &str) -> Decision {
        let c = self.card(card);
        let m = Made {
            conflicts: Vec::new(),
            into_pushed: false,
            tree: c.head.as_ref().map(|h| format!("tree-{h}")),
            commit: c.head,
            contents: c.contents,
            review_changes: None,
        };
        self.answer_job("candidate", |p| of(p, card) && matches!(p, Purpose::Candidate { .. }), json!(m))
    }

    /// The card's suite on its candidate, ending with `exit`.
    pub fn suite(&mut self, card: &str, exit: i32) -> Decision {
        let pick = |p: &Purpose| of(p, card) && matches!(p, Purpose::TrainRun { checks: false, unwalled: false, .. });
        let ids = self.open_jobs(pick);
        let id = ids.first().unwrap_or_else(|| panic!("no suite run; jobs {:?}", self.pending_jobs())).clone();
        let Purpose::TrainRun { commit, .. } = self.purpose(&id) else { unreachable!() };
        let tree = match self.card(card).landing {
            Some(Landing::Test(k) | Landing::Review(k)) => k.tree,
            l => panic!("{card} isn't under test: {l:?}"),
        };
        let at = Checkout { head: Some(commit.clone()), tree, tracked_clean: true, clean: true };
        let run = FinalRun {
            head: commit,
            before: at.clone(),
            after: at,
            checks: None,
            ran: true,
            exit: Some(exit),
            output: format!("SUITE-OUTPUT exit {exit}"),
            phase: Some("walled".into()),
            failing: None,
            logs: Default::default(),
        };
        let d = self.end_job(&id, json!(run));
        self.settle_actions();
        d
    }
}

impl Sim {
    /// The card's clone's import of the base's tip ran, its head as it was: what a join or a failed run's request
    /// waits for (fac-144).
    pub fn imported(&mut self, card: &str) {
        let head = self.card(card).head;
        let status = CloneStatus { branch: card.into(), bundle: head.is_some(), head, clean: true, dirty: Vec::new() };
        let ids = self.open_jobs(|p| of(p, card) && matches!(p, Purpose::Bundle { base_in: Some(_), .. }));
        assert_eq!(ids.len(), 1, "{card} imports nothing; jobs {:?}", self.pending_jobs());
        self.end_job(&ids[0], json!(status));
        self.settle_actions();
    }
}
