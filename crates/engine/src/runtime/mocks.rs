// Mock hosts (fac-258) are kept in memory, as the preview workers are: an app's start starts each again.
// No seat starts or stops one; each runs from its mock step's opening until the card ends.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use factory_core::lifecycle::mock::{FOLDER, HOST, LOG};
use factory_types::{AgentKey, CardId};

use super::Engine;
use crate::jobs::door_ssh::quoted;
use crate::jobs::preview::{self as worker, Worker};
use crate::{clock, random};

/// The hosts running, those left down until the app starts again, and the cards whose mock
/// folder is made.
#[derive(Default)]
pub struct Mocks {
    running: BTreeMap<CardId, Worker>,
    down: BTreeSet<CardId>,
    made: BTreeSet<CardId>,
}

impl Engine {
    /// Ends the hosts of cards whose step is over, marks down those that exited, makes the wanted
    /// cards' mock folders and, with a Vite host set, starts their hosts.
    pub(super) fn mocks_tick(&mut self) {
        let wanted: BTreeMap<CardId, u16> = factory_core::lifecycle::mock::wanted(&self.state).into_iter().collect();
        self.mocks.running.retain(|id, _| wanted.contains_key(id));
        self.mocks.down.retain(|id| wanted.contains_key(id));
        self.mocks.made.retain(|id| wanted.contains_key(id));
        let exited: Vec<(CardId, String)> =
            self.mocks.running.iter_mut().filter_map(|(id, w)| w.exited().map(|s| (id.clone(), s))).collect();
        for (id, status) in exited {
            self.mocks.running.remove(&id);
            self.mocks.down.insert(id.clone());
            self.log(&format!("{id}'s mock host exited ({status}): it stays down until the app starts again"));
        }
        for (id, port) in wanted {
            if self.mocks.running.contains_key(&id) || self.mocks.down.contains(&id) {
                continue;
            }
            match self.mock_start(&id, port) {
                Ok(Some(w)) => {
                    self.mocks.running.insert(id, w);
                }
                Ok(None) => {}
                Err(e) => {
                    self.log(&format!("{id}'s mock host didn't start: {e}"));
                    self.mocks.down.insert(id);
                }
            }
        }
    }

    /// Ends every host: the app stops or execs another build, whose start starts them again.
    pub(super) fn mocks_end(&mut self) {
        self.mocks.running.clear();
    }

    // Its mock folder made, once; then its host, where the factory has a Vite host set.
    fn mock_start(&mut self, id: &CardId, port: u16) -> Result<Option<Worker>, String> {
        let clone = self.clone_dir(id.as_str());
        // A link in its place could take what the engine makes elsewhere (R9).
        if !std::fs::symlink_metadata(clone.join(".git")).is_ok_and(|m| m.is_dir()) {
            return Err(format!("{id} has no clone: its `.git` isn't a folder"));
        }
        if !self.mocks.made.contains(id) {
            factory_sys::mkdir_beneath(&clone, Path::new(FOLDER)).map_err(|e| format!("`{FOLDER}`: {e}"))?;
            self.mocks.made.insert(id.clone());
        }
        let Some(vite) = self.ctx.config.factory.vite.clone() else { return Ok(None) };
        self.mock_spawn(id, &clone, &vite, port).map(Some)
    }

    // PostCSS can execute a mock's config: only the host's own files may be written (fac-499).
    fn mock_spawn(&self, id: &CardId, clone: &Path, vite: &str, port: u16) -> Result<Worker, String> {
        let c = self.card(id)?;
        let config = host_config(clone, vite)?;
        let owner = AgentKey::new(format!("{id}/mock"));
        let now = clock::now_ms();
        let job = factory_core::state::names::job_id(now, &owner, random::salt());
        let toolchains = self.ctx.project_toolchains(Some(c.project.as_str()));
        let mut env = self.ctx.env(&job, &owner, None, &toolchains, &self.opts.home.job(job.as_str()).join("io"));
        // No turn's: the agents' socket knows no job of it.
        for k in ["FACTORY_JOB", "FACTORY_AGENT", "FACTORY_AGENT_SOCK"] {
            env.remove(k);
        }
        env.insert("FACTORY_CARD".into(), id.as_str().to_owned());
        env.insert("FACTORY_PROJECT".into(), c.project.to_string());
        env.insert("FACTORY_PORT".into(), port.to_string());
        env.insert("NODE_PATH".into(), format!("{vite}/node_modules"));
        let places = self.ctx.places(
            &clone.join(HOST),
            vec![clone.display().to_string(), vite.to_owned()],
            Vec::new(),
            None,
            &toolchains,
        )?;
        let cwd = clone.display().to_string();
        // Bundling writes into the read-only clone's node_modules; this plain .mjs needs no transform (fac-499).
        let command = format!(
            "exec {} {} --config {} --configLoader native --host 127.0.0.1 --port {port} --strictPort",
            quoted(&format!("{vite}/node_modules/.bin/vite")),
            quoted(&clone.join(FOLDER).display().to_string()),
            quoted(&config),
        );
        let argv = vec!["sh".into(), "-c".into(), command.clone()];
        let spec = self.ctx.spec(&job, factory_types::job::JobKind::Check, &owner, argv, &cwd, &places, env, None, now);
        worker::start(self.opts.home.root(), &spec, "mock", LOG, &command).map_err(|e| e.to_string())
    }
}

// Vite's own denials but its `.git` one, which would refuse the mock, then all of `.git` but the
// mock's folder; picomatch's `!(factory-mock)` passes any name it begins, hence the last two.
const DENY: &str = r#"[".env", ".env.*", "*.{crt,pem}", "**/.git/!(factory-mock)", "**/.git/!(factory-mock)/**", "**/.git/factory-mock?*", "**/.git/factory-mock?*/**"]"#;

// Vite's watcher ignores `.git`: each mock folder is watched apart, afresh when made again, and relayed to Vite,
// which reads a PostCSS config once and never recompiles CSS for a new file: the relay restarts or restyles.
const CONFIG: &str = r#"import { existsSync, lstatSync, readdirSync, watch } from "node:fs";
import { createRequire } from "node:module";
import { basename, dirname, join, sep } from "node:path";
const root = @MOCK@, web = @WEB@;
const require = createRequire(import.meta.url);
const isPostcss = (name) => name.startsWith("postcss.config.");
const own = () => ["js", "cjs", "mjs", "ts", "cts", "mts"].some((x) => existsSync(join(root, `postcss.config.${x}`)));
const tailwind = () => {
  try { return { plugins: [createRequire(join(web, "package.json"))("@tailwindcss/postcss")({ base: web })] }; } catch {}
};
const relay = { name: "factory-mock-watch", configureServer(server) {
  const watched = new Map();
  let timer, styled;
  const restart = (file) => {
    if (file) delete require.cache[file];
    clearTimeout(timer);
    timer = setTimeout(() => server.restart().catch((e) => server.config.logger.error(String(e))), 100);
  };
  const restyle = () => {
    clearTimeout(styled);
    styled = setTimeout(() => {
      for (const file of server.environments.client.moduleGraph.fileToModulesMap.keys())
        if (file.startsWith(root + sep) && file.endsWith(".css")) server.watcher.emit("change", file);
    }, 100);
  };
  const forget = (dir) => {
    for (const [d, w] of watched) if (d === dir || d.startsWith(dir + sep)) { w.close(); watched.delete(d); }
  };
  const add = (dir, again) => {
    forget(dir);
    let w, entries;
    try {
      w = watch(dir, (event, name) => {
        if (!name) return;
        const file = join(dir, name);
        if (dir === root && isPostcss(name)) return void restart(file);
        let kind;
        try { kind = lstatSync(file); } catch { forget(file); return void server.watcher.emit("unlink", file); }
        if (kind.isDirectory()) add(file, true); else server.watcher.emit("change", file);
        if (event === "rename") restyle();
      });
      entries = readdirSync(dir, { withFileTypes: true });
    } catch { return void w?.close(); }
    w.on("error", () => watched.get(dir) === w && forget(dir));
    watched.set(dir, w);
    for (const e of entries) {
      const path = join(dir, e.name);
      if (e.isDirectory()) add(path, again);
      else if (again && dir === root && isPostcss(e.name)) restart(path);
      else if (again) server.watcher.emit("change", path);
    }
  };
  const had = own();
  let up;
  try {
    up = watch(dirname(root), (_, name) => {
      if (name !== basename(root)) return;
      let kind;
      try { kind = lstatSync(root); } catch {}
      if (kind?.isDirectory()) { add(root, true); restyle(); } else { forget(root); server.watcher.emit("unlink", root); }
      if (had) restart();
    });
    up.on("error", () => {});
  } catch {}
  add(root, false);
  server.httpServer?.once("close", () => { clearTimeout(timer); clearTimeout(styled); up?.close(); watched.forEach((w) => w.close()); });
} };
export default () => {
  const postcss = own() ? undefined : tailwind();
  const modules = join(web, "node_modules");
  const alias = existsSync(join(modules, "react"))
    ? { react: join(modules, "react"), "react-dom": join(modules, "react-dom") }
    : {};
  return {
    cacheDir: @CACHE@,
    plugins: [relay],
    resolve: { alias },
    ...(postcss ? { css: { postcss } } : {}),
    server: { fs: { strict: true, allow: [@CLONE@, @VITE@], deny: @DENY@ } },
  };
};
"#;

// The host's Vite configuration, written beside the mock, never through a link: its cache out of
// the mock's folder, the clone and the Vite folder all it serves files from, `.git` but the mock denied.
fn host_config(clone: &Path, vite: &str) -> Result<String, String> {
    factory_sys::mkdir_beneath(clone, Path::new(HOST)).map_err(|e| format!("`{HOST}`: {e}"))?;
    let path = |p: &Path| serde_json::Value::from(p.display().to_string()).to_string();
    let text = CONFIG
        .replace("@MOCK@", &path(&clone.join(FOLDER)))
        .replace("@WEB@", &path(&clone.join("web")))
        .replace("@CACHE@", &path(&clone.join(HOST).join("cache")))
        .replace("@CLONE@", &path(clone))
        .replace("@VITE@", &path(Path::new(vite)))
        .replace("@DENY@", DENY);
    let file = Path::new(HOST).join("vite.config.mjs");
    factory_sys::write_beneath(clone, &file, text.as_bytes()).map_err(|e| format!("`{}`: {e}", file.display()))?;
    Ok(clone.join(file).display().to_string())
}
