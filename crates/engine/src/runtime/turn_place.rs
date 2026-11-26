use super::*;

impl Engine {
    pub(super) fn turn_place(&self, a: &mork_types::state::Agent) -> Result<TurnPlace, String> {
        use mork_types::reply::Role;
        let mut place = if closing::ending_turn(a) { self.closing_place(a)? } else { self.running_place(a)? };
        let card = mork_core::messages::places::card_of(&self.state, &a.place);
        let key = match card.as_ref() {
            None => a.key.as_str(),
            Some(id) if matches!(a.role, Role::Foreman | Role::Planner | Role::Researcher) => id.as_str(),
            Some(_) => return Ok(place),
        };
        let root = self.opts.home.work();
        crate::seat_homes::mkdir(&root, Path::new(key)).map_err(|e| e.to_string())?;
        place.read_write.push(root.join(key).display().to_string());
        Ok(place)
    }

    // The handoff stays available to seats whose work must leave the clone read-only.
    fn running_place(&self, a: &mork_types::state::Agent) -> Result<TurnPlace, String> {
        let card = mork_core::messages::places::card_of(&self.state, &a.place).and_then(|c| self.state.cards.get(&c));
        let Some(card) = card else {
            let work = self.opts.home.work().join(a.key.as_str());
            return Ok(TurnPlace { cwd: work.display().to_string(), ..TurnPlace::default() });
        };
        let clone = self.opts.home.root().join("clones").join(card.id.as_str());
        if !clone.is_dir() {
            return Err(format!("{} has no clone", card.id));
        }
        let clone_s = clone.display().to_string();
        // A foreman's researcher reads the clone from a folder of its own (fac-94).
        let researches = a.parent.is_some() && a.role == mork_types::reply::Role::Researcher;
        let own = self.opts.home.work().join(a.key.as_str());
        let (mut read_only, read_write) = if researches {
            crate::seat_homes::mkdir(&self.opts.home.work(), Path::new(a.key.as_str())).map_err(|e| e.to_string())?;
            (vec![clone_s.clone()], Vec::new())
        } else if a.role == mork_types::reply::Role::Planner && mork_core::lifecycle::mock::writes(card) {
            (vec![clone_s.clone()], vec![handoff_dir(&clone)?, mock_dir(&clone)?])
        } else if a.role == mork_types::reply::Role::Baseline {
            // The baseline seat writes the mock's folder alone, never the handoff (fac-636).
            (vec![clone_s.clone()], vec![mock_dir(&clone)?])
        } else if a.role == mork_types::reply::Role::Planner
            || (card.mode == mork_types::card::Mode::Report && a.role == mork_types::reply::Role::Foreman)
        {
            (vec![clone_s.clone()], vec![handoff_dir(&clone)?])
        } else if mork_core::lifecycle::facts::clone_writable(card) {
            (Vec::new(), vec![clone_s.clone()])
        } else {
            (vec![clone_s.clone()], Vec::new())
        };
        let cwd = if researches { own.display().to_string() } else { clone_s.clone() };
        let mut env = BTreeMap::from([
            ("MORK_CARD".to_owned(), card.id.as_str().to_owned()),
            ("MORK_PROJECT".to_owned(), card.project.to_string()),
        ]);
        if researches {
            env.insert("MORK_CLONE".into(), clone_s);
        }
        let project = self.ctx.config.projects.get(&card.project);
        // The checks folder's checkout, read-only, for `mork test --checks` (4.9.1, fac-165).
        read_only.extend(project.and_then(|p| p.checks.as_ref()).map(|k| self.checks_mount(&k.folder)));
        if let Some(port) = card.port {
            env.insert("MORK_PORT".into(), port.to_string());
        }
        let toolchains = project.map(|p| p.toolchains.clone()).unwrap_or_default();
        let data = project.and_then(|p| p.data.clone());
        Ok(TurnPlace { cwd, read_only, read_write, env, toolchains, empty: None, data })
    }

    // Where a closing turn runs: where its session ran, as Claude finds a session by its folder
    // (fac-147). A clone's path, the clone there or not; whether it is one.
    pub(super) fn closing_cwd(&self, a: &mork_types::state::Agent) -> (PathBuf, bool) {
        let researcher = a.role == mork_types::reply::Role::Researcher;
        match mork_core::messages::places::card_of(&self.state, &a.place) {
            Some(c) if !(researcher && a.parent.is_some()) => (self.clone_dir(c.as_str()), true),
            _ => (self.opts.home.work().join(a.key.as_str()), false),
        }
    }

    // Nothing of a clone: walled, an empty tmpfs at its path; else the folder `closing_dir` made
    // where it is gone, or the clone as it is.
    fn closing_place(&self, a: &mork_types::state::Agent) -> Result<TurnPlace, String> {
        let (dir, clone) = self.closing_cwd(a);
        let cwd = dir.display().to_string();
        if clone {
            return Ok(TurnPlace { cwd: cwd.clone(), empty: Some(cwd), ..TurnPlace::default() });
        }
        crate::seat_homes::mkdir(&self.opts.home.work(), Path::new(a.key.as_str())).map_err(|e| e.to_string())?;
        Ok(TurnPlace { cwd, ..TurnPlace::default() })
    }

    // An unwalled closing turn needs its gone clone's path on the host: an empty folder, kept
    // until no turn is in it.
    pub(super) fn closing_dir(&mut self, key: &AgentKey) -> std::io::Result<()> {
        let Some(a) = self.state.agents.get(key).filter(|a| a.retiring.is_some()) else { return Ok(()) };
        let (dir, clone) = self.closing_cwd(a);
        if clone && !dir.exists() {
            std::fs::create_dir_all(&dir)?;
            self.closing_dirs.insert(dir);
        }
        Ok(())
    }

    // Removed once no job runs in it, so nothing takes it for a clone; one a clone was made in
    // since, its `.git` there, is left alone.
    pub(super) fn sweep_closing_dirs(&mut self) {
        let busy: Vec<PathBuf> =
            self.state.jobs.values().filter(|j| j.settled.is_none()).map(|j| PathBuf::from(&j.spec.cwd)).collect();
        self.closing_dirs.retain(|d| {
            if busy.contains(d) {
                return true;
            }
            if !d.join(".git").exists() {
                let _ = std::fs::remove_dir_all(d);
            }
            false
        });
    }
}

// Made beneath the clone through no link, so the wall mounts a real folder and `.git` is never made.
fn handoff_dir(clone: &Path) -> Result<String, String> {
    let handoff = Path::new(mork_core::memory::HANDOFF);
    for rel in [handoff.parent().unwrap_or(handoff), handoff] {
        mork_sys::mkdir_beneath(clone, rel).map_err(|e| format!("{}: {e}", clone.join(rel).display()))?;
    }
    Ok(clone.join(handoff).display().to_string())
}

// The open mock's writable mount must be a real folder, never a link planted by another seat.
fn mock_dir(clone: &Path) -> Result<String, String> {
    let rel = Path::new(mork_core::lifecycle::mock::FOLDER);
    mork_sys::mkdir_beneath(clone, rel).map_err(|e| format!("{}: {e}", clone.join(rel).display()))?;
    Ok(clone.join(rel).display().to_string())
}
