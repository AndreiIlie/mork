// fac-480: the engine's exception to the wall is bounded by the requesting seat and literal test words.
#![cfg(feature = "faults")]
// why: assertions in a wall-boundary test may fail with the input that broke the boundary.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use factory_core::decide::{Ctx, Decision};
use factory_core::messages::places::Who;
use factory_core::state::State;
use factory_types::card::{Card, Mode};
use factory_types::config::Config;
use factory_types::reply::Role;
use factory_types::state::Sender;

fn request(config: &Config, project: &str, mode: Mode, role: Role, words: &[&str]) -> Decision {
    let mut state = State::default();
    let card = Card::new("fac-1".into(), project, "Wall test permissions", mode, 1, 0);
    state.cards.insert(card.id.clone(), card);
    let name = if role == Role::Foreman { "foreman".into() } else { format!("{}-1", role.as_str()) };
    let key = format!("fac-1/{name}");
    let place = if role == Role::Foreman { "fac-1".into() } else { format!("{key}/dm") };
    let agent =
        factory_core::post::new_agent(&Who { key: key.into(), seat: role.as_str().into(), role, place: place.into() });
    let from = Sender::Agent(agent.key.clone());
    state.agents.insert(agent.key.clone(), agent);
    let command: Vec<String> = words.iter().map(|w| (*w).into()).collect();
    factory_core::queue::leases::lease(&state, &from, "walls", &command, Ctx { now: 1, config, faults: true })
}

#[test]
fn fac480_wall_test_requests_respect_the_wall_boundary() {
    let mut config = factory_engine::config::parse(
        "[factory]\nproject = \"factory\"\n[git]\nauthor = \"A <a@example.invalid>\"\n\
         [projects.factory]\nrepo = \"/repo\"\nboard = \"/board\"\ntest = \"true\"\n\
         [projects.other]\nrepo = \"/other\"\nboard = \"/other-board\"\ntest = \"true\"\n",
    )
    .unwrap();
    config.factory.max_test_runs = 0;
    for (project, mode, role) in [
        ("other", Mode::Full, Role::Foreman),
        ("factory", Mode::Review, Role::Foreman),
        ("factory", Mode::Review, Role::Reviewer),
        ("factory", Mode::Full, Role::Researcher),
        ("factory", Mode::Report, Role::Foreman),
        ("factory", Mode::Full, Role::Echo),
    ] {
        let d = request(&config, project, mode, role, &["t1_walls"]);
        assert!(matches!(d.answer, Some(Err(_))), "{project} {mode:?} {role:?}: {d:?}");
        assert!(d.changes.is_empty() && d.intents.is_empty(), "{d:?}");
    }
    let invalid: &[&[&str]] = &[
        &[],
        &["--no-default-features"],
        &["../t1_walls"],
        &["/t1_walls"],
        &["--all-targets"],
        &["t1_*"],
        &["t1_walls.rs"],
        &[""],
        &["t1 walls"],
        &["t1_walls", "--all-targets"],
        &["t1_walls", "one two"],
        &["t1_walls", ""],
        &["t1_walls", "one", "two"],
        &["--no-default-features", "t1_walls", "--features=faults"],
    ];
    for words in invalid {
        let d = request(&config, "factory", Mode::Full, Role::Foreman, words);
        assert!(matches!(d.answer, Some(Err(_))), "{words:?}: {d:?}");
        assert!(d.changes.is_empty() && d.intents.is_empty(), "{d:?}");
    }
    config.factory.max_test_runs = 1;
    for words in [&["t1_walls"][..], &["--no-default-features", "t1_walls", "production"][..]] {
        let d = request(&config, "factory", Mode::Full, Role::Foreman, words);
        assert!(matches!(d.answer, Some(Ok(ref v)) if v["lease"]["walls"] == true), "{d:?}");
    }
}
