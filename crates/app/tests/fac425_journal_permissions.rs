#![cfg(feature = "faults")]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::door::{answers, ctl_turn};
use common::{CODEX_FOREMAN, Rig, reply};
use factory_core::decide::Journal;
use factory_types::card::{Mode, Stage};
use serde_json::json;

#[test]
fn journal_read_holds_the_seats_project_limit() {
    let mut rig = Rig::new();
    let repo = rig.t.root.join("factory-repo").display().to_string();
    let extra = format!(
        "[projects.first]\nrepo = \"{repo}\"\nboard = \"{repo}\"\ntest = \"true\"\n\
         [projects.second]\nrepo = \"{repo}\"\nboard = \"{repo}\"\ntest = \"true\"\n{CODEX_FOREMAN}"
    );
    rig.write_config(&rig.config(&extra));
    common::builds::factory_repo(&rig, 1);
    let mut limited = rig.card("hf-1", Mode::Full);
    limited.project = "first".into();
    let mut target = rig.card("hf-2", Mode::Full);
    target.project = "second".into();
    target.stage = Stage::Done;
    let broad = rig.card("fac-1", Mode::Full);
    rig.put_cards(vec![limited, target, broad]);
    let data = json!({"stage": "done", "paused": false});
    rig.put_journaled(
        vec![],
        &[Journal { card: Some("hf-2".into()), kind: "card.stage".into(), data: data.clone() }],
        42,
    );
    let calls = [["history.journal", "hf-2", "1", "--at", "42", "--kinds", "card.stage"].map(str::to_owned).to_vec()];
    rig.scenario(json!([
        ctl_turn("hf-1/foreman", Some("Read the journal"), &calls, reply("working", "Checked", json!([]))),
        ctl_turn("fac-1/foreman", Some("Read the journal"), &calls, reply("working", "Read", json!([]))),
    ]));
    rig.start();
    rig.say("hf-1", "@foreman Read the journal");
    rig.wait_turns("hf-1/foreman", 1);
    let refused = answers(&rig, &rig.jobs("hf-1/foreman").remove(0));
    assert_eq!(refused.len(), 1, "{refused:#?}");
    assert_eq!(refused[0]["err"]["code"], "refused", "{refused:#?}");
    rig.say("fac-1", "@foreman Read the journal");
    rig.wait_turns("fac-1/foreman", 1);
    let allowed = answers(&rig, &rig.jobs("fac-1/foreman").remove(0));
    assert_eq!(allowed.len(), 1, "{allowed:#?}");
    assert_eq!(allowed[0]["history.journal"]["rows"][0]["data"], data, "{allowed:#?}");
}
