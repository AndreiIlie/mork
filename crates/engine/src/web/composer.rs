// Local composer actions answer on the WebSocket without reaching the operator socket.

use mork_types::Command;
use mork_types::catalog::{CATALOG, slash_fields};
use mork_types::input::{self, Parsed};
use mork_types::protocol::{ComposerAction, ComposerRequest, ErrorBody, ErrorCode, MAX_LINE};
use serde_json::json;

pub(super) enum Destination {
    Socket(String),
    Browser(String),
}

pub(super) fn route(text: String) -> Destination {
    let Ok(req) = serde_json::from_str::<ComposerRequest>(&text) else { return Destination::Socket(text) };
    let draft = req.composer;
    let id = req.id;
    let refuse = |code, message: String| {
        Destination::Browser(json!({"id": id, "err": ErrorBody::new(code, message)}).to_string())
    };
    let parsed = input::submission(&draft.text, &draft.images, &draft.context, draft.now)
        .unwrap_or_else(|note| Parsed::Note(note.into()));
    let action = match parsed {
        Parsed::Send(Command::Watch { .. }) => {
            return refuse(ErrorCode::Refused, "Use `mork ctl watch` in a terminal to watch a place.".into());
        }
        Parsed::Send(cmd) => {
            let line = json!({"id": id, "cmd": cmd}).to_string();
            if line.len() > MAX_LINE {
                return refuse(ErrorCode::TooLarge, "the line is over 1 MiB".into());
            }
            return Destination::Socket(line);
        }
        Parsed::Open(card) => {
            if let Some(note) = input::open_refusal(&card) {
                return refuse(ErrorCode::Invalid, note);
            }
            ComposerAction::Open { card }
        }
        Parsed::Help => ComposerAction::Help { commands: help() },
        Parsed::Usage(_) => ComposerAction::Usage,
        Parsed::Note(message) => return refuse(ErrorCode::Invalid, message),
        Parsed::Nothing => return Destination::Browser(json!({"id": id, "ok": {}}).to_string()),
    };
    Destination::Browser(json!({"id": id, "ok": {"composer": action}}).to_string())
}

fn help() -> Vec<(String, String)> {
    let mut commands: Vec<_> = CATALOG
        .iter()
        .filter(|spec| !spec.faults_only && !matches!(spec.name, "usage" | "watch"))
        .map(|spec| (format!("/{}", spec.name), slash_fields(spec)))
        .collect();
    commands.sort_unstable();
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn frame(text: &str, now: bool, images: &[&str]) -> Destination {
        route(
            json!({"id": 7, "composer": {"text": text, "now": now, "images": images,
            "context": {"place": "ex-1/spec", "card": "ex-1", "to": "planner"}}})
            .to_string(),
        )
    }

    fn sent(text: &str, now: bool, images: &[&str]) -> Value {
        let Destination::Socket(line) = frame(text, now, images) else { panic!("expected an engine command") };
        serde_json::from_str(&line).unwrap()
    }

    fn local(text: &str, now: bool) -> Value {
        let Destination::Browser(line) = frame(text, now, &[]) else { panic!("expected a browser response") };
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["id"], 7);
        answer
    }

    #[test]
    fn messages_and_images_keep_their_destination_and_delivery() {
        assert_eq!(
            sent(" @foreman  hello ", true, &["a"]),
            json!({"id":7,"cmd":{"say":{
            "place":"ex-1/spec","to":"foreman","text":"hello","images":["a"],"delivery":"now"}}})
        );
        assert_eq!(
            sent("", false, &["a"])["cmd"]["say"],
            json!({"place":"ex-1/spec","to":"planner","text":"","images":["a"]})
        );
    }

    #[test]
    fn slash_commands_keep_native_defaults_quoting_and_integer_precision() {
        assert_eq!(
            sent("/answer 18446744073709551615 'two words'", false, &["a"]),
            json!({"id":7,"cmd":{
            "answer":{"card":"ex-1","n":u64::MAX,"text":"two words"}}})
        );
        assert_eq!(sent("/say hello", false, &[])["cmd"]["say"], json!({"place":"ex-1/spec","text":"hello"}));
        assert_eq!(sent("/approve ex-2", false, &[])["cmd"]["approve"], json!({"card":"ex-2"}));
    }

    #[test]
    fn typed_watch_leaves_the_browsers_subscription_alone() {
        for text in ["/watch", "/watch main", "/watch screen ex-1 --tail 20 --full"] {
            let error = &local(text, false)["err"];
            assert_eq!(error["code"], "refused");
            assert!(error["message"].as_str().unwrap().contains("mork ctl watch"));
        }
        let watch = r#"{"id":1,"cmd":{"watch":{"places":["screen","ex-1"],"tail":100,"full":true}}}"#;
        assert!(matches!(route(watch.into()), Destination::Socket(line) if line == watch));
    }

    #[test]
    fn navigation_help_and_errors_answer_without_dispatching() {
        assert_eq!(local("/open ex-2", false)["ok"]["composer"], json!({"action":"open","card":"ex-2"}));
        assert_eq!(local("/usage", false)["ok"]["composer"], json!({"action":"usage"}));
        let help = local("/help", false);
        let commands = help["ok"]["composer"]["commands"].as_array().unwrap();
        assert!(commands.contains(&json!(["/approve", "<card>"])));
        assert!(!commands.iter().any(|row| row[0] == "/watch"));
        assert!(!commands.iter().any(|row| row[0] == "/debug.sleep"));
        for (text, now) in [
            ("/demo", true),
            ("/open", false),
            ("/open ex-1/challenge", false),
            ("/say 'hi", false),
            ("@ ", false),
            ("@foreman", false),
            ("/debug.sleep 2", false),
        ] {
            assert_eq!(local(text, now)["err"]["code"], "invalid");
        }
        let original = r#"{"id":5,"cmd":{"status":{}}}"#;
        assert!(matches!(route(original.to_owned()), Destination::Socket(line) if line == original));
    }
}
