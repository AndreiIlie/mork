mod common;

use common::Sim;
use mork_core::decide::buzz::buzz;
use mork_types::Command;
use mork_types::config::view::ConfigChange;
use mork_types::protocol::ErrorCode;
use mork_types::state::Sender;

#[test]
fn settings_commands_belong_to_the_operator() {
    let sim = Sim::new();
    let change = ConfigChange { section: "limits".into(), project: None, fields: Default::default() };
    for command in [Command::Config {}, Command::ConfigChange { change }] {
        assert!(buzz(&sim.state, &Sender::Operator, &command, sim.ctx()).is_none());
        for sender in
            [Sender::Agent("ex-1/foreman".into()), Sender::Orchestrator("web".into()), Sender::Engine, Sender::Pa]
        {
            assert_eq!(buzz(&sim.state, &sender, &command, sim.ctx()).map(|e| e.code), Some(ErrorCode::Buzz));
        }
    }
}
