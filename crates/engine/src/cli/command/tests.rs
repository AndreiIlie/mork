use serde_json::json;

use super::display;
use crate::cli::{StreamState, agy, appserver, claude, codex, turn};

#[test]
fn labels_describe_the_invocation_and_keep_its_original_spelling() {
    for (raw, label) in [
        ("cat crates/core/lib.rs", "Read lib.rs"),
        (r#"/bin/bash -c "sed -n '58,125p' crates/core/act_found.rs""#, "Read act_found.rs (58-125)"),
        ("rg -n 'held_words' crates/core", "Search held_words in crates/core"),
        ("grep -rin 'two words' 'my files'", "Search two words in my files"),
        ("mork ctl card.show fac-751", "Read card notes"),
        ("git status --short", "Check working tree status"),
        ("git diff -- crates", "Show file changes"),
        ("/bin/bash -lc 'git diff --stat'", "Summarize file changes"),
        ("mork ctl memory.search 'door timing'", "Search memory for door timing"),
        ("mork ctl memory.read factory wall-tests", "Read wall-tests from factory memory"),
        ("/bin/bash -c 'pwd'", "pwd"),
        ("/bin/sh -c 'git status && pwd'", "git status && pwd"),
        ("/bin/bash -c 'echo one\necho two'", "echo one"),
        ("rg --files", "rg --files"),
    ] {
        let (words, command) = display(None, raw, None);
        assert_eq!(words, label);
        let command = command.unwrap();
        assert_eq!(command.text, raw);
        assert!(!command.cut);
    }
}

#[test]
fn every_stream_applies_masking_before_publishing_command_details() {
    for protected in [false, true] {
        let raw = if protected { "/bin/bash -c 'mork ctl say fac-751/spec private body'" } else { "git diff --stat" };
        let shell_label = if protected { "Sent a message to the planner" } else { "Summarize file changes" };
        let description = "A description supplied by Claude";
        let item = json!({"id": "1", "type": "commandExecution", "command": raw});
        let tool = json!({"type": "tool_use", "id": "1", "name": "Bash", "input": {"command": raw, "description": description}});
        let cli_events = [
            (
                appserver::read_line as fn(&mut StreamState, &str),
                json!({"method":"item/started","params":{"item":item}}),
                shell_label,
            ),
            (
                codex::read_line,
                json!({"type":"item.completed","item":{"type":"command_execution","id":"1","command":raw}}),
                shell_label,
            ),
            (
                agy::read_line,
                json!({"event":"step_update","step_update":{"step_type":"tool","step_index":1,"tool_info":{"parameters":{"CommandLine":raw}}}}),
                shell_label,
            ),
            (
                claude::read_line,
                json!({"type":"assistant","message":{"content":[tool]}}),
                if protected { shell_label } else { description },
            ),
        ];
        for (read, event, label) in cli_events {
            let mut st = StreamState { seat: Some("fac-751/foreman".into()), ..Default::default() };
            read(&mut st, &event.to_string());
            let view = turn::turn_of(&st, &Default::default());
            assert_eq!(view.steps.len(), 1);
            assert_eq!(view.steps[0].words, label);
            assert_eq!(view.steps[0].command.as_ref().map(|c| c.text.as_str()), (!protected).then_some(raw));
        }
    }
}
