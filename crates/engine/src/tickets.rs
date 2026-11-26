use std::collections::BTreeMap;
use std::sync::Arc;

use crate::lock::Lock;

use mork_types::config::{ProjectConfig, TicketsConfig};
use mork_types::work::{OpenPr, Ticket};

pub(crate) enum Read {
    Ticket(Ticket),
    Missing,
    Unavailable,
}

pub(crate) trait TicketsRead {
    fn get(&self, id: u64) -> Read;
}

fn reader(config: &TicketsConfig) -> Option<Box<dyn TicketsRead>> {
    match config {
        TicketsConfig::AzureDevops { api, token, .. } => Some(Box::new(crate::azure::Client::new(api, token)?)),
    }
}

type Tickets = (String, Vec<Ticket>);

/// What each open PR's tickets were last read for: its head and the numbers it referred to.
#[derive(Debug, Default, Clone)]
pub struct Cache(Arc<Lock<BTreeMap<(String, u64), Tickets>>>);

impl Cache {
    /// The PRs with the tickets they refer to, read again for a new head or new numbers; a PR whose
    /// tickets couldn't be read has none until a later poll reads them. Read only for projects with `tickets`.
    pub fn attach(&self, project: &str, config: &ProjectConfig, mut prs: Vec<OpenPr>) -> Vec<OpenPr> {
        let Some(tickets_cfg) = &config.tickets else { return prs };
        let Some(reader) = reader(tickets_cfg) else { return prs };
        let mut seen = self.0.lock();
        seen.retain(|(p, n), _| p != project || prs.iter().any(|x| !x.fork && x.pr == *n));
        let mut down = false;
        for pr in prs.iter_mut().filter(|pr| !pr.fork) {
            let numbers =
                mork_core::reviews::tickets::candidates(&pr.title, &pr.description, &pr.branch, tickets_cfg.max_id());
            let version = format!("{} {numbers:?}", pr.head);
            let key = (project.to_owned(), pr.pr);
            if let Some((_, tickets)) = seen.get(&key).filter(|(v, _)| *v == version) {
                pr.tickets.clone_from(tickets);
                continue;
            }
            let mut tickets = Vec::new();
            let mut read_all = true;
            for n in numbers {
                // Azure being down for this poll, no other PR is tried: each call may wait out its timeout.
                if down {
                    read_all = false;
                    break;
                }
                match reader.get(n) {
                    Read::Ticket(t) => tickets.push(t),
                    Read::Missing => {}
                    Read::Unavailable => {
                        down = true;
                        read_all = false;
                        break;
                    }
                }
            }
            if read_all {
                seen.insert(key, (version, tickets.clone()));
                pr.tickets = tickets;
            }
        }
        prs
    }
}
