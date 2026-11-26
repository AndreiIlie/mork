// Version 56 links the asks before replacing the old text they identify.
use std::collections::BTreeSet;

use mork_types::ask::{Ask, AskKind};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::json;

use super::Result;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    let mut rows = tx.prepare("SELECT data FROM entities WHERE kind='ask' AND json_extract(data,'$.open')")?;
    let asks = rows.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut used = BTreeSet::new();
    for data in asks {
        let raw = serde_json::from_str(&data)?;
        if super::retired::step_ask(&raw) {
            continue;
        }
        let ask: Ask = serde_json::from_value(raw)?;
        let seq = match source(tx, &ask)?.filter(|seq| used.insert(*seq)) {
            Some(seq) => seq,
            None => replacement(tx, &ask)?,
        };
        tx.execute(
            "UPDATE entities SET data=json_set(data,'$.seq',?1) WHERE kind='ask' AND id=?2",
            params![seq, ask.id.as_str()],
        )?;
    }
    tx.execute_batch(
        "UPDATE messages SET data=json_set(data,
        '$.hand.message', json_extract(data,'$.hand.message') ||
            CASE WHEN json_extract(data,'$.hand.message')<>'' AND json_extract(data,'$.hand.ask')<>''
                 THEN char(10,10) ELSE '' END || json_extract(data,'$.hand.ask'),
        '$.text', json_extract(data,'$.text') ||
            CASE WHEN json_extract(data,'$.text')<>'' AND json_extract(data,'$.hand.ask')<>''
                 THEN char(10,10) ELSE '' END || json_extract(data,'$.hand.ask'),
        '$.hand.ask', json(CASE WHEN json_extract(data,'$.hand.ask')<>'' THEN 'true' ELSE 'false' END))
        WHERE json_type(data,'$.hand.ask')='text'",
    )?;
    Ok(())
}

fn source(tx: &Transaction<'_>, ask: &Ask) -> Result<Option<u64>> {
    if ask.kind == AskKind::Triage {
        return Ok(tx
            .query_row(
                "SELECT seq FROM messages WHERE place=?1 AND json_extract(data,'$.time')=?2
            AND json_extract(data,'$.from')='engine' AND json_extract(data,'$.headline') IN
            ('findings wait for triage','review stalemate waits for triage') GROUP BY place HAVING count(*)=1",
                params![ask.place.as_str(), ask.raised_ms],
                |r| r.get(0),
            )
            .optional()?);
    }
    let outcome = match ask.kind {
        AskKind::Answer => None,
        AskKind::Done => Some("report-ready"),
        AskKind::Blocked => Some("blocked"),
        AskKind::Mock | AskKind::ApproveBrief => Some("brief-ready"),
        _ => return Ok(None),
    };
    // The journal binds the outcome to its hand even if a later reply asked another question.
    let mut query = tx.prepare(
        "SELECT m.seq, m.data FROM events e JOIN messages m
        ON m.seq=json_extract(e.data,'$.hand') WHERE e.kind='reply' AND e.time=?1
        AND (e.card IS ?2) AND (?3 IS NULL OR json_extract(e.data,'$.agent')=?3)
        AND (?4 IS NULL OR json_extract(e.data,'$.outcome')=?4)",
    )?;
    let mut rows = query.query(params![ask.raised_ms, ask.card.as_deref(), ask.agent.as_deref(), outcome])?;
    let mut found = None;
    while let Some(row) = rows.next()? {
        let data: String = row.get(1)?;
        let m: serde_json::Value = serde_json::from_str(&data)?;
        if !m["hand"].is_object() || (ask.kind == AskKind::Answer && !matches_answer(ask, &m["hand"])) {
            continue;
        }
        if found.is_some() {
            return Ok(None);
        }
        found = Some(row.get(0)?);
    }
    Ok(found)
}

fn matches_answer(a: &Ask, hand: &serde_json::Value) -> bool {
    let question = a.choices.as_ref().map_or(a.text.as_str(), |c| c.ask.as_str());
    hand["ask"].as_str() == Some(question)
        || (hand["ask"] == "" && hand["color"] == "red" && hand["withdraw_ask"] != true && hand["headline"] == a.text)
}

fn replacement(tx: &Transaction<'_>, a: &Ask) -> Result<u64> {
    let seat: Option<String> = if a.kind == AskKind::Answer {
        tx.query_row(
            "SELECT json_extract(data,'$.place') FROM entities WHERE kind='agent' AND id=?1",
            [a.agent.as_deref()],
            |r| r.get(0),
        )
        .optional()?
    } else {
        None
    };
    let place = seat
        .as_deref()
        .or_else(|| a.agent.as_deref().filter(|k| a.kind == AskKind::Answer && k.ends_with("/dm")))
        .unwrap_or(a.place.as_str());
    let seq: u64 = tx.query_row("SELECT coalesce(max(seq)+1,0) FROM messages", [], |r| r.get(0))?;
    let headline = mork_core::messages::asks::word(a.kind);
    let data = json!({"seq":seq,"place":place,"time":a.raised_ms,"from":"engine","to":"operator",
        "kind":"engine","headline":headline,"text":a.text,"images":[],"findings":[]});
    tx.execute("INSERT INTO messages(seq,place,data) VALUES(?1,?2,?3)", params![seq, place, data.to_string()])?;
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mork_types::ask::Color;
    use mork_types::state::Message;
    use rusqlite::Connection;
    use serde_json::Value;

    #[test]
    fn report_question_and_missing_dm_keep_separate_messages() -> Result<()> {
        let mut db = Connection::open_in_memory()?;
        let tx = db.transaction()?;
        super::super::schema::create(&tx)?;
        for (seq, outcome, body, question) in
            [(1, "report-ready", "The report", ""), (2, "working", "Context", "Proceed?")]
        {
            let hand = json!({"message":body, "ask":question, "color":"grey", "headline":"Result"});
            let msg = json!({"seq":seq,"place":"card","time":seq,"from":{"agent":"card/foreman"},
                "to":"operator","kind":"hand","headline":"Result","text":body,"hand":hand,"images":[],"findings":[]});
            tx.execute("INSERT INTO messages VALUES(?1,'card',?2)", params![seq, msg.to_string()])?;
            let event = json!({"agent":"card/foreman","outcome":outcome,"hand":seq});
            tx.execute(
                "INSERT INTO events(time,card,kind,data) VALUES(?1,'card','reply',?2)",
                params![seq, event.to_string()],
            )?;
        }
        for (id, kind, time, agent, words) in [
            ("report", AskKind::Done, 1, None, "Report ready"),
            ("question", AskKind::Answer, 2, Some("card/foreman"), "Proceed?"),
            ("dm", AskKind::Answer, 3, Some("card/foreman/dm"), "Which one?"),
        ] {
            let mut a = Ask::new(id.into(), "card", Some("card".into()), kind, Color::Yellow, words);
            a.agent = agent.map(Into::into);
            a.raised_ms = time;
            let mut data = serde_json::to_value(a)?;
            if let Value::Object(ref mut fields) = data {
                fields.remove("seq");
            }
            tx.execute("INSERT INTO entities VALUES('ask',?1,?2)", params![id, data.to_string()])?;
        }
        migrate(&tx)?;
        for (id, seq, place, words) in [
            ("report", 1, "card", "The report"),
            ("question", 2, "card", "Context\n\nProceed?"),
            ("dm", 3, "card/foreman/dm", "Which one?"),
        ] {
            let sql = "SELECT m.data FROM entities e JOIN messages m ON m.seq=json_extract(e.data,'$.seq')
                WHERE e.kind='ask' AND e.id=?1";
            let data: String = tx.query_row(sql, [id], |r| r.get(0))?;
            let m: Message = serde_json::from_str(&data)?;
            assert_eq!((m.seq, m.place.as_str(), m.text.as_str()), (seq, place, words));
            if let Some(h) = m.hand {
                assert_eq!(h.ask, id == "question");
                assert_eq!(h.message, words);
            }
        }
        Ok(())
    }
}
