// What a start loads (fac-336): live work whole; of ended cards, only the numbers and keys new
// records need, read from ids and status columns, so no history row is parsed.

use std::collections::{BTreeMap, BTreeSet};

use factory_core::state::State;
use factory_core::state::unloaded::{Records, companions};
use factory_types::action::{Action, ActionStatus};
use factory_types::card::Card;
use factory_types::state::{Job, Owed};
use factory_types::{ActionId, CardId, JobId};
use rusqlite::{OptionalExtension, params};

use super::{Result, Store, load_entity, retired};

// The entity kinds kept per card; every other kind is small and loaded whole.
const PER_CARD: &str = "('agent', 'place', 'card', 'ask')";

// A done or dropped action's id, natural key and status.
type Settled = (String, String, ActionStatus);

// The newest builds' jobs: master's tip is among them, which a failed build isn't tried again for (fac-19).
const BUILDS_KEPT: usize = 20;

// Every card id, and those to load.
struct Chosen {
    all: BTreeSet<String>,
    load: BTreeSet<String>,
}

impl Chosen {
    // The card a record's id names first (`<card>/...`), if it is a card's.
    fn card_of<'a>(&self, id: &'a str) -> Option<&'a str> {
        id.split('/').next().filter(|c| self.all.contains(*c))
    }

    // A record of a card loaded, or of no card.
    fn wanted(&self, id: &str) -> bool {
        self.card_of(id).is_none_or(|c| self.load.contains(c))
    }

    fn add(&mut self, card: Option<&CardId>) {
        if let Some(c) = card.filter(|c| self.all.contains(c.as_str())) {
            self.load.insert(c.as_str().to_owned());
        }
    }
}

impl Store {
    /// Loads live work whole (fac-336): live cards, ended ones something still runs for, and the
    /// places of no card; of the rest, what new records must not reuse (`State::unloaded`).
    pub fn load(&self) -> Result<State> {
        let mut s = State { revision: self.revision()?, ..State::default() };
        for (kind, data) in
            self.rows(&format!("SELECT kind, data FROM entities WHERE kind NOT IN {PER_CARD} ORDER BY kind, id"))?
        {
            load_entity(&mut s, &kind, &data)?;
        }
        let mut chosen = self.cards_chosen()?;
        let unsettled = self.unsettled_jobs()?;
        let (unfinished, settled) = self.actions_by_status()?;
        for j in &unsettled {
            chosen.add(job_card(&chosen, j).as_ref());
        }
        for a in &unfinished {
            chosen.add(a.what.card());
        }
        for w in s.owed_work.values() {
            chosen.add(w.card());
        }
        for l in s.leases.values() {
            chosen.add(l.card.as_ref());
        }
        self.load_entities(&mut s, &chosen)?;
        self.load_jobs(&mut s, &chosen, unsettled)?;
        self.load_actions(&mut s, &chosen, unfinished, settled)?;
        self.load_messages(&mut s, &chosen)?;
        s.unloaded.cards = chosen.all.difference(&chosen.load).map(CardId::new).collect();
        let more: BTreeSet<CardId> =
            s.cards.values().flat_map(companions).filter(|c| s.unloaded.cards.contains(c)).collect();
        for (id, r) in self.cards_records(&more)? {
            s.absorb(&id, r);
        }
        self.load_carrying(&mut s)?;
        s.next_seq = self.top("SELECT max(seq) FROM messages")?.map_or(0, |t| t + 1);
        s.orchestrator_next = self.orchestrator_next()?;
        retired::drop_research(&mut s);
        Ok(s)
    }

    // Live cards, and ended ones with a seat in its closing turn or an open ask.
    fn cards_chosen(&self) -> Result<Chosen> {
        let mut chosen = Chosen { all: BTreeSet::new(), load: BTreeSet::new() };
        let sql = "SELECT id, json_extract(data, '$.stage') FROM entities WHERE kind = 'card'";
        for (id, stage) in self.pairs(sql)? {
            if stage != "done" && stage != "closed" {
                chosen.load.insert(id.clone());
            }
            chosen.all.insert(id);
        }
        let held = "SELECT id, '' FROM entities WHERE (kind = 'agent' AND json_extract(data, '$.retiring') IS NOT NULL) \
                    OR (kind = 'ask' AND json_extract(data, '$.open'))";
        for (id, _) in self.pairs(held)? {
            if let Some(c) = chosen.card_of(&id) {
                chosen.load.insert(c.to_owned());
            }
        }
        Ok(chosen)
    }

    // A closed run holds the cards it carries until its work lands or its card is closed on its board (fac-563): its
    // card record alone, the rest of it read as any ended card's is, when something names it.
    fn load_carrying(&self, s: &mut State) -> Result<()> {
        let sql = "SELECT id, data FROM entities WHERE kind = 'card' AND json_extract(data, '$.stage') = 'closed' \
                   AND json_array_length(data, '$.carries') > 0";
        for (id, data) in self.rows(sql)? {
            if s.cards.contains_key(&CardId::new(id)) {
                continue;
            }
            let c: Card = serde_json::from_str(&data)?;
            if factory_core::lifecycle::carried::holding(s, &c) {
                s.cards.insert(c.id.clone(), c);
            }
        }
        Ok(())
    }

    fn unsettled_jobs(&self) -> Result<Vec<Job>> {
        let mut jobs = Vec::new();
        for (_, data) in self.rows("SELECT id, data FROM jobs WHERE status <> 'settled'")? {
            if let Some(j) = job(&data)? {
                jobs.push(j);
            }
        }
        Ok(jobs)
    }

    // The actions not done nor dropped, whole; the others' ids, keys and statuses.
    fn actions_by_status(&self) -> Result<(Vec<Action>, Vec<Settled>)> {
        let mut unfinished = Vec::new();
        let mut settled = Vec::new();
        let mut stmt = self.conn.prepare("SELECT id, natural_key, status FROM actions")?;
        let rows =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        for row in rows {
            let (id, key, status) = row?;
            match status.as_str() {
                "done" => settled.push((id, key, ActionStatus::Done)),
                "dropped" => settled.push((id, key, ActionStatus::Dropped)),
                _ => unfinished.extend(self.action(&id)?),
            }
        }
        Ok((unfinished, settled))
    }

    fn load_entities(&self, s: &mut State, chosen: &Chosen) -> Result<()> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM entities WHERE kind = ?1 AND id = ?2")?;
        for kind in ["card", "agent", "place", "ask"] {
            for id in self.column(&format!("SELECT id FROM entities WHERE kind = '{kind}'"))? {
                if kind == "ask" {
                    count_ask(s, &id);
                }
                if chosen.wanted(&id)
                    && let Some(data) = stmt.query_row(params![kind, id], |r| r.get::<_, String>(0)).optional()?
                {
                    load_entity(s, kind, &data)?;
                }
            }
        }
        Ok(())
    }

    fn load_jobs(&self, s: &mut State, chosen: &Chosen, unsettled: Vec<Job>) -> Result<()> {
        for j in unsettled {
            s.jobs.insert(j.id.clone(), j);
        }
        let mut tried = BTreeSet::new();
        let mut wanted = self.jobs_wanted(s, chosen)?;
        // An owed retry's request is read at its launch, however old (fac-626).
        wanted.extend(s.agents.values().filter_map(|a| match &a.owed {
            Some(Owed::Retry { request, .. }) => request.clone(),
            _ => None,
        }));
        // The jobs a loaded one corrects or retries: what its logical turn is (fac-1); a short retry's request.
        while !wanted.is_empty() {
            for id in std::mem::take(&mut wanted) {
                if tried.insert(id.clone()) && !s.jobs.contains_key(&id) {
                    s.jobs.extend(self.job_of(&id)?.map(|j| (j.id.clone(), j)));
                }
            }
            let earlier = s.jobs.values().flat_map(|j| [&j.corrects, &j.retry_of, &j.continues]).flatten();
            wanted = earlier.filter(|id| !s.jobs.contains_key(*id) && !tried.contains(*id)).cloned().collect();
        }
        Ok(())
    }

    // A loaded card's jobs; of no card, each seat's latest, which its resume reads, and those whose
    // folder upkeep still keeps, which a chat's turns read; and the newest builds'.
    fn jobs_wanted(&self, s: &State, chosen: &Chosen) -> Result<Vec<JobId>> {
        let seats: BTreeSet<String> = s
            .agents
            .keys()
            .filter(|k| chosen.card_of(k.as_str()).is_none())
            .map(|k| k.as_str().replace('/', "-"))
            .collect();
        let (mut wanted, mut theirs, mut builds) = (Vec::new(), BTreeMap::<String, Vec<_>>::new(), Vec::new());
        let mut newest = 0;
        for (id, settled) in self.ids_settled()? {
            newest = newest.max(settled.unwrap_or(0));
            match (job_id_card(chosen, &id), job_owner(&id)) {
                (Some(c), _) if chosen.load.contains(c) => wanted.push(JobId::new(id)),
                (None, Some("factory-build")) => builds.push(id),
                (None, Some(owner)) if seats.contains(owner) => {
                    theirs.entry(owner.to_owned()).or_default().push((id, settled));
                }
                _ => {}
            }
        }
        let kept = |t: Option<i64>| t.is_none_or(|t| t >= newest - factory_core::maintenance::upkeep::KEEP_JOBS);
        for mut jobs in theirs.into_values() {
            let latest = jobs.pop();
            wanted.extend(jobs.into_iter().filter(|(_, t)| kept(*t)).chain(latest).map(|(id, _)| JobId::new(id)));
        }
        wanted.extend(builds.into_iter().rev().take(BUILDS_KEPT).map(JobId::new));
        Ok(wanted)
    }

    fn ids_settled(&self) -> Result<Vec<(String, Option<i64>)>> {
        let mut stmt = self.conn.prepare("SELECT id, settled_ms FROM jobs ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn job_of(&self, id: &JobId) -> Result<Option<Job>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM jobs WHERE id = ?1")?;
        let data: Option<String> = stmt.query_row(params![id.as_str()], |r| r.get(0)).optional()?;
        Ok(data.map(|d| job(&d)).transpose()?.flatten())
    }

    fn load_actions(
        &self,
        s: &mut State,
        chosen: &Chosen,
        unfinished: Vec<Action>,
        settled: Vec<Settled>,
    ) -> Result<()> {
        let mut top = 0;
        for a in unfinished {
            s.actions.insert(a.id.clone(), a);
        }
        let owed = self.owed_doors()?;
        let mut theirs = BTreeSet::new();
        for card in &chosen.load {
            theirs.extend(self.action_ids_of(card)?);
        }
        for (id, key, status) in settled {
            top = top.max(number(&id));
            // A door call in a turn still loaded: its turn's dedup reads it (fac-1).
            let turn = door_turn(&key).is_some_and(|t| s.jobs.contains_key(&JobId::new(t)));
            if (theirs.contains(&id) || turn)
                && let Some(a) = self.action(&id)?
            {
                s.actions.insert(a.id.clone(), a);
                continue;
            }
            // A call whose news is owed: `door_recover` tells it after a crash.
            if owed.contains(&id)
                && let Some(a) = self.action(&id)?
                && factory_core::door::owed_news(&a)
            {
                s.actions.insert(a.id.clone(), a);
                continue;
            }
            s.unloaded.actions.insert(key, (factory_types::ActionId::new(id), status));
        }
        s.unloaded.action_top = s.actions.keys().map(|k| number(k.as_str())).fold(top, u64::max);
        Ok(())
    }

    fn load_messages(&self, s: &mut State, chosen: &Chosen) -> Result<()> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM messages WHERE place = ?1 ORDER BY seq")?;
        for place in self.column("SELECT DISTINCT place FROM messages")? {
            if chosen.wanted(&place) {
                for data in stmt.query_map(params![place], |r| r.get::<_, String>(0))? {
                    s.messages.push(serde_json::from_str(&data?)?);
                }
            }
        }
        s.messages.sort_by_key(|m| m.seq);
        Ok(())
    }

    /// Ended cards' records, as the store holds them, for `State::absorb` (fac-336).
    pub fn cards_records(&self, cards: &BTreeSet<CardId>) -> Result<Vec<(CardId, Records)>> {
        if cards.is_empty() {
            return Ok(Vec::new());
        }
        let all = self.column("SELECT id FROM entities WHERE kind = 'card'")?.into_iter().collect();
        let chosen = Chosen { all, load: cards.iter().map(|c| c.as_str().to_owned()).collect() };
        let mut jobs: BTreeMap<String, Vec<Job>> = BTreeMap::new();
        for id in self.column("SELECT id FROM jobs")? {
            if let Some(c) = job_id_card(&chosen, &id).filter(|c| chosen.load.contains(*c)) {
                jobs.entry(c.to_owned()).or_default().extend(self.job_of(&JobId::new(id.as_str()))?);
            }
        }
        let mut out = Vec::new();
        for card in cards {
            let mut r = self.card_records(card)?;
            r.jobs = jobs.remove(card.as_str()).unwrap_or_default();
            out.push((card.clone(), r));
        }
        Ok(out)
    }

    // A card's records but its jobs.
    fn card_records(&self, card: &CardId) -> Result<Records> {
        let mut r = Records::default();
        let id = card.as_str();
        let range = "SELECT data FROM entities WHERE kind = ?1 AND (id = ?2 OR (id > ?2 || '/' AND id < ?2 || '0'))
                     AND (kind <> 'agent' OR json_extract(data, '$.role') IS NOT 'builder')";
        let mut stmt = self.conn.prepare_cached(range)?;
        let mut of = |kind: &str| -> Result<Vec<String>> {
            Ok(stmt.query_map(params![kind, id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        };
        let mut s = State::default();
        for kind in ["card", "agent", "place", "ask"] {
            for data in of(kind)? {
                load_entity(&mut s, kind, &data)?;
            }
        }
        r.card = s.cards.remove(card);
        r.agents = s.agents.into_values().collect();
        r.asks = s.asks.into_values().collect();
        let mut messages = self.conn.prepare_cached("SELECT data FROM messages WHERE place = ?1 ORDER BY seq")?;
        for p in s.places.into_values() {
            for data in messages.query_map(params![p.id.as_str()], |r| r.get::<_, String>(0))? {
                r.messages.push(serde_json::from_str(&data?)?);
            }
            r.places.push(p);
        }
        r.messages.sort_by_key(|m| m.seq);
        for a in self.action_ids_of(id)? {
            r.actions.extend(self.action(&a)?);
        }
        Ok(r)
    }

    /// A card's title and the base moves and merges that landed it, for the changelog of a card the
    /// state doesn't hold (fac-336).
    pub fn landing_of(&self, card: &str) -> Result<(Option<String>, Vec<Action>)> {
        let sql = "SELECT json_extract(data, '$.title') FROM entities WHERE kind = 'card' AND id = ?1";
        let title = self.conn.query_row(sql, params![card], |r| r.get(0)).optional()?;
        let landing = "SELECT data FROM actions WHERE id = ?1 \
                       AND (natural_key LIKE 'move-base:%' OR natural_key LIKE 'merge:%')";
        let mut stmt = self.conn.prepare_cached(landing)?;
        let mut actions = Vec::new();
        for id in self.action_ids_of(card)? {
            if let Some(data) = stmt.query_row(params![id], |r| r.get::<_, String>(0)).optional()? {
                actions.push(serde_json::from_str(&data)?);
            }
        }
        Ok((title, actions))
    }

    /// The actions of these ids the store holds.
    pub fn actions_of(&self, ids: &BTreeSet<ActionId>) -> Result<Vec<Action>> {
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.action(id.as_str())?);
        }
        Ok(out)
    }

    // A card's actions, by the journal line each was made with (`action`, its card's).
    fn action_ids_of(&self, card: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM events WHERE card = ?1 AND kind = 'action'")?;
        let rows = stmt.query_map(params![card], |r| r.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for data in rows {
            let v: serde_json::Value = serde_json::from_str(&data?)?;
            ids.extend(v.get("id").and_then(serde_json::Value::as_str).map(str::to_owned));
        }
        Ok(ids)
    }

    fn action(&self, id: &str) -> Result<Option<Action>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM actions WHERE id = ?1")?;
        let data: Option<String> = stmt.query_row(params![id], |r| r.get(0)).optional()?;
        Ok(data.map(|d| serde_json::from_str(&d)).transpose()?)
    }

    fn column(&self, sql: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(sql)?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    fn pairs(&self, sql: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn top(&self, sql: &str) -> Result<Option<u64>> {
        let v: Option<i64> = self.conn.query_row(sql, [], |r| r.get(0))?;
        Ok(v.and_then(|v| u64::try_from(v).ok()))
    }
}

// A job's record, unless it is a retired research's (fac-312).
fn job(data: &str) -> Result<Option<Job>> {
    let v: serde_json::Value = serde_json::from_str(data)?;
    if v.get("purpose").is_some_and(retired::research_purpose) {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(v)?))
}

fn job_card(chosen: &Chosen, j: &Job) -> Option<CardId> {
    j.purpose.as_ref().and_then(|p| p.card().cloned()).or_else(|| chosen.card_of(j.owner.as_str()).map(CardId::new))
}

// A job's id is `<yyyymmdd-hhmmss>-<owner>-<salt>`, the owner's `/` made `-`: its card is the
// longest card id it begins with, followed by a `-`.
fn job_id_card<'a>(chosen: &Chosen, id: &'a str) -> Option<&'a str> {
    let rest = id.get(16..)?;
    rest.match_indices('-').map(|(i, _)| &rest[..i]).rfind(|c| chosen.all.contains(*c))
}

// The owner a job's id names, its `/` made `-`.
fn job_owner(id: &str) -> Option<&str> {
    id.get(16..id.len().checked_sub(5)?)
}

// The logical turn a door call's key names (`door:<sender>:<turn>:<request>`).
fn door_turn(key: &str) -> Option<&str> {
    key.strip_prefix("door:")?.rsplit(':').nth(1)
}

fn number(id: &str) -> u64 {
    id.strip_prefix("act-").and_then(|n| n.parse().ok()).unwrap_or(0)
}

// An ask's id is `<place>/<kind>/<n>`: its number is not reused.
fn count_ask(s: &mut State, id: &str) {
    if let Some((prefix, n)) = id.rsplit_once('/')
        && let Ok(n) = n.parse::<u64>()
    {
        let top = s.unloaded.asks.entry(prefix.to_owned()).or_default();
        *top = (*top).max(n);
    }
}
