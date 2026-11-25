// fac-664: ending a card stops running start and train jobs once, blocks revival, and still releases train clones.

mod common;

use common::{Sim, sha};
use factory_core::decide::{Event, Intent, JobEnd, SessionSeen};
use factory_types::action::ActionWhat;
use factory_types::card::Stage;
use factory_types::work::Purpose;
use serde_json::json;

// why: a missing card in the start response is a test failure.
#[allow(clippy::unwrap_used)]
fn begin_lookback(s: &mut Sim, project: &str) -> String {
    s.andrei(json!({"lookback": {"project": project}})).unwrap()["lookback"]["card"].as_str().unwrap().to_owned()
}

fn assert_stayed_done(s: &mut Sim, id: &str) {
    assert_eq!(s.stage(id), Stage::Done);
    assert!(s.card(id).failed_job.is_none());
    assert_eq!(s.card(id).job_cuts, 0);
    assert!(s.open_jobs(|p| p.card().is_some_and(|c| c.as_str() == id)).is_empty());
    assert!(s.state.owed_work.values().all(|w| w.card().is_none_or(|c| c.as_str() != id)));
    assert!(s.pending().is_empty());
    assert!(!s.owed(&format!("{id}/foreman")));
    assert!(s.state.asks.values().all(|a| !a.open));
}

#[test]
fn done_stops_lookback_clone_and_blocks_revival_with_or_without_stop() {
    let mut s = Sim::new();
    s.config.projects.get_mut("web").unwrap().install = Some("true".into());
    for unstopped in [false, true] {
        let lb = begin_lookback(&mut s, "web");
        s.settle_actions();
        let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card, .. } if card.as_str() == lb))[0].clone();
        s.intents.clear();
        s.andrei(json!({"done": {"card": lb}})).unwrap();
        assert_eq!(s.intents.iter().filter(|i| **i == Intent::StopJob { id: clone.clone() }).count(), 1);
        assert!(s.state.jobs[&clone].stop_requested);
        s.settle_actions();
        if unstopped {
            s.state.jobs.get_mut(&clone).unwrap().stop_requested = false;
        }
        s.end_job(&clone, json!({"lookback": {}}));
        assert_stayed_done(&mut s, &lb);
    }
    let next = begin_lookback(&mut s, "web");
    assert_eq!(s.stage(&next), Stage::Starting);
}

#[test]
fn done_blocks_report_revival_and_retries_on_clone_or_install_end() {
    let mut s = Sim::new();
    s.config.projects.get_mut("web").unwrap().install = Some("true".into());
    let cases = [
        (false, true, JobEnd::Finished { reply: Ok(json!({})), session: SessionSeen::default() }),
        (true, false, JobEnd::Finished { reply: Ok(json!({})), session: SessionSeen::default() }),
        (true, true, JobEnd::Finished { reply: Ok(json!({})), session: SessionSeen::default() }),
        (true, true, JobEnd::Cut { session: SessionSeen::default() }),
        (true, true, JobEnd::Failed { words: "boom".into() }),
    ];
    for (i, (on_install, unstopped, end)) in cases.into_iter().enumerate() {
        let id = format!("web-{}", i + 10);
        s.board(&id, "web");
        s.andrei(json!({"start": {"card": id, "mode": "report"}})).unwrap();
        s.settle_actions();
        let mut job = s.open_jobs(|p| matches!(p, Purpose::Clone { card, .. } if card.as_str() == id))[0].clone();
        if on_install {
            s.end_job(&job, json!({}));
            job = s.open_jobs(|p| matches!(p, Purpose::Install { card, .. } if card.as_str() == id))[0].clone();
        }
        s.intents.clear();
        s.andrei(json!({"done": {"card": id}})).unwrap();
        assert_eq!(s.intents.iter().filter(|i| **i == Intent::StopJob { id: job.clone() }).count(), 1);
        assert!(s.state.jobs[&job].stop_requested);
        s.settle_actions();
        if unstopped {
            s.state.jobs.get_mut(&job).unwrap().stop_requested = false;
        }
        s.apply(Event::JobEnded { id: job, end });
        assert_stayed_done(&mut s, &id);
    }
}

#[test]
fn ending_card_with_running_train_job_stops_once_and_releases_train_clone() {
    let mut s = Sim::new();
    let rc = s.andrei(json!({"review.branch": {"project": "web", "branch": "feat"}})).unwrap()["review.branch"]["card"]
        .as_str()
        .unwrap()
        .to_owned();
    let fetch = s.action_of(|w| matches!(w, ActionWhat::Fetch { project, .. } if project.as_str() == "web"));
    s.returned(&fetch.id, Ok(json!({"base_tip": sha("base1"), "tips": {"feat": sha("h1")}})));
    let cand = s.open_jobs(|p| matches!(p, Purpose::Candidate { card, .. } if card.as_str() == rc))[0].clone();
    let Purpose::Candidate { clone, .. } = s.purpose(&cand) else { panic!("expected candidate job") };
    s.intents.clear();
    s.andrei(json!({"done": {"card": rc}})).unwrap();
    assert_eq!(s.intents.iter().filter(|i| **i == Intent::StopJob { id: cand.clone() }).count(), 1);
    assert!(!s.has_action(|w| matches!(w, ActionWhat::RemoveTrain { card, .. } if card.as_str() == rc)));
    s.settle_actions();
    s.end_job(&cand, json!({}));
    assert!(
        s.has_action(|w| matches!(w, ActionWhat::RemoveTrain { card, clone: c } if card.as_str() == rc && *c == clone))
    );
    assert_eq!(s.stage(&rc), Stage::Done);
    assert!(s.card(&rc).failed_job.is_none());

    s.through_brief("web-1", "web");
    s.built("web-1", "h1", "base1");
    s.checks("web-1", 0, &[]);
    let full_cand =
        s.open_jobs(|p| matches!(p, Purpose::Candidate { card, .. } if card.as_str() == "web-1"))[0].clone();
    let Purpose::Candidate { clone: full_clone, .. } = s.purpose(&full_cand) else {
        panic!("expected candidate job");
    };
    s.intents.clear();
    s.andrei(json!({"close": {"card": "web-1", "why": "dropped"}})).unwrap();
    assert_eq!(s.intents.iter().filter(|i| **i == Intent::StopJob { id: full_cand.clone() }).count(), 1);
    assert!(!s.has_action(|w| matches!(w, ActionWhat::RemoveTrain { card, .. } if card.as_str() == "web-1")));
    s.end_job(&full_cand, json!({}));
    assert!(s.has_action(
        |w| matches!(w, ActionWhat::RemoveTrain { card, clone: c } if card.as_str() == "web-1" && *c == full_clone)
    ));
    assert_eq!(s.stage("web-1"), Stage::Closed);
}
