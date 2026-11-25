// The token's verdict (3.7.2) read by the action threads right before each Bitbucket call, under the project's lock
// (2.5.8, M4.5); a token file changed since Bitbucket was last asked about is asked about first, and the call waits.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use factory_core::decide::Event;

use super::Input;
use crate::bitbucket::calls;
use crate::bitbucket::client::{BitbucketError, Repo};

#[derive(Debug, Default)]
struct Inner {
    refused: Option<String>,
    followed: Option<String>,
    asked: Option<String>,
}

/// Shared by the engine, the poller and the action threads.
#[derive(Debug, Default, Clone)]
pub struct Verdict(Arc<Mutex<Inner>>);

impl Verdict {
    /// The state's verdict, followed when it changes.
    pub fn follow(&self, refused: Option<&String>) {
        let Ok(mut v) = self.0.lock() else { return };
        if v.followed.as_ref() != refused {
            v.followed = refused.cloned();
            v.refused = refused.cloned();
        }
    }

    /// Bitbucket said who the token of this file's text is.
    pub fn asked(&self, text: &str, refused: Option<String>) {
        let Ok(mut v) = self.0.lock() else { return };
        v.asked = Some(crate::files::sha256_hex(text.as_bytes()));
        v.refused = refused;
    }

    /// Bitbucket was asked about this file's text, and couldn't say (`GET /user` refused, 3.7.2):
    /// the verdict stands.
    pub fn unsaid(&self, text: &str) {
        let Ok(mut v) = self.0.lock() else { return };
        v.asked = Some(crate::files::sha256_hex(text.as_bytes()));
    }

    /// Why the token file's `text`, as the guard read it once, may not act, if so. A text Bitbucket wasn't
    /// asked about is asked about here with that very token, and the engine told what it said (M5.1).
    pub fn now(&self, repo: &Repo, text: &str, tx: &Sender<Input>) -> Result<Option<String>, String> {
        let token = crate::token::parse(text).map_err(|e| e.to_string())?;
        let hash = crate::files::sha256_hex(text.as_bytes());
        if let Ok(v) = self.0.lock()
            && v.asked.as_deref() == Some(hash.as_str())
        {
            return Ok(v.refused.clone());
        }
        let send = |e: Event| {
            let _ = tx.send(Input::Event(Box::new(e)));
        };
        match calls::identity(repo, token.approver.as_deref()) {
            Ok(refused) => {
                self.asked(text, refused.clone());
                send(Event::TokenRead { refused: refused.clone() });
                Ok(refused)
            }
            Err(BitbucketError::Answered(a)) if a.status == 403 => {
                self.unsaid(text);
                send(Event::TokenAccepted);
                Ok(self.0.lock().ok().and_then(|v| v.refused.clone()))
            }
            Err(e) => Err(format!("the token file changed, and Bitbucket couldn't be asked whose it is: {e}")),
        }
    }
}
