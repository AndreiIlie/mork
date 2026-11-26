//! A card's plan (fac-36, fac-128): `deliverables` cuts it and `plan.task` marks its current task, waking nobody;
//! `plan` posts one deliverable's tasks to the foreman, waking it only for the operator or an orchestrator (fac-577).

use mork_types::card::{Card, Deliverable, Plan};
use mork_types::command::{Pick, TextList};
use mork_types::reply::{Intent, Role};
use mork_types::state::{MessageKind, Sender};
use mork_types::{CardId, Command};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::messages::places::{self, Who};
use crate::post::Poster;
use crate::state::{Change, State};

// A plan's bounds: it shows whole in the window, and its view stays within its line.
const TASKS_MAX: usize = 100;
const TASK_CHARS_MAX: usize = 400;
// A cut's bounds: each deliverable a section of the window's plan, its name its heading.
const DELIVERABLES_MAX: usize = 30;
const DELIVERABLE_CHARS_MAX: usize = 200;

fn live(state: &State, card: &CardId) -> Result<Card, String> {
    let id = CardId::new(card.as_str().to_lowercase());
    match state.cards.get(&id) {
        Some(c) if c.stage.is_live() => Ok(c.clone()),
        Some(c) => Err(format!("{id} is {}: its plan no longer moves", c.stage.as_str())),
        None => Err(format!("no card `{id}`")),
    }
}

// A card whose plan is read: with a foreman to read it.
fn read(state: &State, c: &Card) -> Result<Who, String> {
    places::card_agent(state, &c.id, Role::Foreman)
}

// Both wire forms keep the same task boundaries and list-marker handling (fac-417).
fn tasks(input: &TextList) -> Vec<String> {
    let items = match input {
        TextList::Text(text) => serde_json::from_str::<Vec<String>>(text.trim())
            .unwrap_or_else(|_| text.lines().map(str::to_owned).collect()),
        TextList::Items(items) => items.clone(),
    };
    items.iter().map(|t| unmarked(t)).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

fn unmarked(line: &str) -> &str {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(['-', '*', '•']).filter(|r| r.is_empty() || r.starts_with(' ')) {
        return rest.trim_start();
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if digits > 0 && (rest.is_empty() || rest.starts_with(' ')) => rest.trim_start(),
        _ => line,
    }
}

fn checked(tasks: &[String]) -> Result<(), String> {
    if tasks.is_empty() {
        return Err("a plan needs a task: one per line".into());
    }
    if tasks.len() > TASKS_MAX {
        return Err(format!("a plan holds at most {TASKS_MAX} tasks, not {}", tasks.len()));
    }
    match tasks.iter().position(|t| t.chars().count() > TASK_CHARS_MAX) {
        Some(i) => Err(format!("task {} is over {TASK_CHARS_MAX} characters: say it shorter", i + 1)),
        None => Ok(()),
    }
}

// A deliverable is known by its name from cut to cut: two of one name would share their tasks.
fn names_checked(names: &[String]) -> Result<(), String> {
    if names.is_empty() {
        return Err("a card needs a deliverable: one name per line".into());
    }
    if names.len() > DELIVERABLES_MAX {
        return Err(format!("a card holds at most {DELIVERABLES_MAX} deliverables, not {}", names.len()));
    }
    if let Some(n) = names.iter().find(|n| n.chars().count() > DELIVERABLE_CHARS_MAX) {
        return Err(format!("`{n}` is over {DELIVERABLE_CHARS_MAX} characters: name it shorter"));
    }
    match names.iter().enumerate().find(|(i, n)| names[..*i].contains(n)) {
        Some((_, n)) => Err(format!("`{n}` is named twice: each deliverable needs a name of its own")),
        None => Ok(()),
    }
}

/// `deliverables`, `plan` and `plan.task`; any other command is refused.
pub(super) fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::Deliverables { card, names } = cmd {
        cut(state, from, card, names, ctx)
    } else if let Command::Plan { card, deliverable, tasks } = cmd {
        publish(state, from, card, deliverable, tasks, ctx)
    } else if let Command::PlanTask { card, deliverable, task } = cmd {
        self::task(state, card, deliverable, task)
    } else {
        Decision::refused(format!("`{}` isn't a plan's command", cmd.name()))
    }
}

fn cut(state: &State, from: &Sender, card: &CardId, text: &TextList, ctx: Ctx<'_>) -> Decision {
    let mut c = match live(state, card) {
        Ok(c) => c,
        Err(e) => return Decision::refused(e),
    };
    let names = tasks(text);
    if let Err(e) = names_checked(&names).and_then(|()| read(state, &c).map(|_| ())) {
        return Decision::refused(e);
    }
    let was = c.plan.take().unwrap_or_default();
    let deliverables = names
        .iter()
        .map(|n| {
            let kept = was.deliverables.iter().find(|d| d.name == *n).cloned();
            kept.unwrap_or_else(|| Deliverable { name: n.clone(), tasks: Vec::new(), current: 0 })
        })
        .collect();
    let current = was.deliverable().and_then(|d| names.iter().position(|n| *n == d.name)).unwrap_or(0);
    let plan = Plan { deliverables, current };
    // A new current deliverable's task is the one worked on from now, or none: the journal says which.
    let now = plan.deliverable();
    let moved = (now.map(|d| &d.name) != was.deliverable().map(|d| &d.name)).then(|| {
        let task = now.and_then(|d| Some((d, d.tasks.get(d.current)?)));
        task.map_or(Value::Null, |(d, text)| json!({"deliverable": d.name, "task": d.current + 1, "text": text}))
    });
    c.plan = Some(plan);
    let listed: Vec<String> = names.iter().map(|n| format!("- {n}")).collect();
    let words = format!("deliverables: {}\n{}", names.len(), listed.join("\n"));
    let mut p = Poster::for_ctx(state, ctx);
    let seq = p.post(c.id.as_str(), from.clone(), None, MessageKind::Update, "", &words);
    if let (Some(m), Sender::Agent(_)) = (p.last_message(), from) {
        m.intent = Some(Intent::Update);
    }
    p.put_card(c.clone());
    let mut data = json!({"names": names, "from": from.label(), "seq": seq});
    if let Some(now) = moved {
        data["current"] = now;
    }
    p.journal("deliverables", Some(c.id.as_str()), data);
    let mut d = Decision::ok(json!({"deliverables": {"card": c.id.as_str(), "deliverables": names.len(), "seq": seq}}));
    p.finish(&mut d);
    d
}

// Names are unique in a plan, so an exact name picks one; digits alone stay a number (fac-255).
fn index_of(plan: &Plan, pick: &Pick) -> Result<usize, String> {
    let of = plan.deliverables.len();
    if let Some(n) = pick.number() {
        return usize::try_from(n)
            .ok()
            .filter(|n| (1..=of).contains(n))
            .map(|n| n - 1)
            .ok_or_else(|| format!("the card's deliverables are 1 to {of}, not {n}"));
    }
    let Pick::Name(name) = pick else { return Err("a deliverable is a number or a name".into()) };
    let name = name.trim();
    plan.deliverables.iter().position(|d| d.name == name).ok_or_else(|| {
        let names: Vec<String> = plan.deliverables.iter().map(|d| format!("\"{}\"", d.name)).collect();
        format!("the card has no deliverable \"{name}\": its deliverables are {}", names.join(", "))
    })
}

/// The deliverable `pick` names, as `plan` takes it: a card cut into none has only the whole card's, which naming
/// it makes.
pub(crate) fn deliverable_of(plan: &mut Option<Plan>, pick: &Pick) -> Result<usize, String> {
    let of = plan.as_ref().map_or(0, |p| p.deliverables.len());
    if of == 0 {
        return match pick.number() {
            Some(1) => {
                *plan = Some(Plan::whole_card(Vec::new(), 0));
                Ok(0)
            }
            _ => Err("the card has no deliverables: cut them with `deliverables`, or plan 1".to_owned()),
        };
    }
    plan.as_ref().map_or(Err("no plan".into()), |p| index_of(p, pick))
}

// Task texts repeat, so a text two tasks share is refused rather than picked silently (fac-255).
fn task_of(name: &str, tasks: &[String], pick: &Pick) -> Result<usize, String> {
    let of = tasks.len();
    if let Some(n) = pick.number() {
        return usize::try_from(n)
            .ok()
            .filter(|n| (1..=of).contains(n))
            .map(|n| n - 1)
            .ok_or_else(|| format!("the tasks of \"{name}\" are 1 to {of}, not {n}"));
    }
    let Pick::Name(text) = pick else { return Err("a task is a number or its text".into()) };
    let text = text.trim();
    let at: Vec<usize> = tasks.iter().enumerate().filter(|(_, t)| *t == text).map(|(i, _)| i + 1).collect();
    match at.as_slice() {
        [only] => Ok(only - 1),
        [] => Err(format!("\"{name}\" has no task \"{text}\": its tasks are 1 to {of}")),
        many => {
            let numbers: Vec<String> = many.iter().map(usize::to_string).collect();
            Err(format!("\"{name}\" has tasks {} reading \"{text}\": say one by its number", numbers.join(", ")))
        }
    }
}

// `plan <card> <deliverable> <tasks>`: its tasks replaced, its first current and itself current, posted in the card's
// room to the foreman: an agent's for its next news (fac-577), the operator's or an orchestrator's waking it as their words do.
fn publish(state: &State, from: &Sender, card: &CardId, n: &Pick, text: &TextList, ctx: Ctx<'_>) -> Decision {
    let mut c = match live(state, card) {
        Ok(c) => c,
        Err(e) => return Decision::refused(e),
    };
    let tasks = tasks(text);
    let foreman = match checked(&tasks).and_then(|()| read(state, &c)) {
        Ok(w) => w,
        Err(e) => return Decision::refused(e),
    };
    let i = match deliverable_of(&mut c.plan, n) {
        Ok(i) => i,
        Err(e) => return Decision::refused(e),
    };
    let Some(plan) = c.plan.as_mut() else { return Decision::refused(format!("{} has no plan", c.id)) };
    plan.current = i;
    let it = &mut plan.deliverables[i];
    it.tasks = tasks;
    it.current = 0;
    let (name, steps) = (it.name.clone(), it.tasks.clone());
    let numbered: Vec<String> = steps.iter().enumerate().map(|(i, t)| format!("{}. {t}", i + 1)).collect();
    let s = if steps.len() == 1 { "" } else { "s" };
    let words = format!("plan: {} task{s} for \"{name}\"\n{}", steps.len(), numbered.join("\n"));
    let wakes = !matches!(from, Sender::Agent(_));
    let kind = if wakes { MessageKind::Request } else { MessageKind::Update };
    let mut p = Poster::for_ctx(state, ctx);
    let seq = p.post(c.id.as_str(), from.clone(), Some("foreman"), kind, "", &words);
    // A plan informs and wants nothing back (fac-18): an agent's is an update, the operator's has no kind.
    if let (Some(m), Sender::Agent(_)) = (p.last_message(), from) {
        m.intent = Some(Intent::Update);
    }
    if wakes {
        p.wake(&foreman, from);
    }
    p.put_card(c.clone());
    p.journal(
        "plan",
        Some(c.id.as_str()),
        json!({"deliverable": name, "tasks": steps.len(), "steps": steps, "from": from.label(), "seq": seq}),
    );
    let mut d =
        Decision::ok(json!({"plan": {"card": c.id.as_str(), "deliverable": i + 1, "tasks": steps.len(), "seq": seq}}));
    p.finish(&mut d);
    d
}

// `plan.task <card> <deliverable> <task>`: that deliverable's task, by number from 1 or by its text,
// is the one worked on now, and the deliverable the current one. It posts nothing.
fn task(state: &State, card: &CardId, deliverable: &Pick, pick: &Pick) -> Decision {
    let mut c = match live(state, card) {
        Ok(c) => c,
        Err(e) => return Decision::refused(e),
    };
    let Some(plan) = c.plan.as_mut() else {
        return Decision::refused(format!("{} has no plan: publish it with `plan`", c.id));
    };
    let at = match index_of(plan, deliverable) {
        Ok(at) => at,
        Err(e) => return Decision::refused(e),
    };
    let it = &mut plan.deliverables[at];
    let of = it.tasks.len();
    if of == 0 {
        return Decision::refused(format!("\"{}\" has no tasks: publish them with `plan`", it.name));
    }
    let i = match task_of(&it.name, &it.tasks, pick) {
        Ok(i) => i,
        Err(e) => return Decision::refused(e),
    };
    it.current = i;
    let (name, text) = (it.name.clone(), it.tasks[i].clone());
    plan.current = at;
    let mut d =
        Decision::ok(json!({"plan.task": {"card": c.id.as_str(), "deliverable": at + 1, "task": i + 1, "of": of}}));
    d.journal("plan.task", Some(c.id.as_str()), json!({"deliverable": name, "task": i + 1, "of": of, "text": text}));
    d.changes.push(Change::PutCard(Box::new(c)));
    d
}
