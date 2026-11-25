# Security Model & Reporting

Mork runs autonomous coding CLIs with interactive permission prompts disabled inside a Linux sandbox. The sandbox wall, symlink-safe host file operations, and out-of-wall door are the security boundaries.

## What Mork Enforces Today

- **Fail-closed Linux sandbox (`crates/engine/src/jobs/wall.rs`, `wall_exec.rs`, `crates/sys/src/landlock.rs`):**
  - Runs each turn under Bubblewrap with `--unshare-user`, `--unshare-pid`, `--unshare-ipc`, `--cap-drop ALL`, and `--clearenv`.
  - Mounts the host root read-only (`--ro-bind / /`), covers host home directories, runtime dirs, and Mork's own state/sockets/keys with tmpfs, and exposes only the card's clone (writable for build seats, read-only for researchers and reviewers) and explicit `toolchains` / `hide` rules from config.
  - Installs a Landlock TCP connect filter before exec and refuses to run in production builds if the kernel lacks Landlock support. Test-only wall overrides (`FACTORY_UNWALLED`, `FACTORY_TEST_*`) compile out when the `faults` feature is disabled.
- **Symlink escape protection (`crates/sys/`):**
  - The engine never runs `git` using an agent-writable clone as its repository and accesses paths inside clones only through `*_beneath` (`openat`-rooted) helpers so a symlink planted by a seat cannot redirect host writes.
- **Out-of-wall credentials & redaction (`crates/engine/src/door/`, `crates/engine/src/secrets.rs`):**
  - Database, SSH, and API tokens stay outside the sandbox wall. Door operations run on the host side against `targets.toml` allowlists, require explicit operator approval for `misc.run`, and scrub secrets (including JSON-escaped forms) from logs and output.
- **Local web bridge (`crates/engine/src/web/key.rs`, `crates/engine/src/web/http.rs`):**
  - Binds `127.0.0.1` only, authenticates requests with a 32-byte mode-`0600` key passed in the URL fragment using constant-time comparison, and enforces an `Origin` allowlist.

## Known Limitations (On the Public Roadmap)

- **Network egress:** The network namespace is not unshared because CLIs connect to vendor HTTPS endpoints. Landlock restricts TCP connect ports (default `443`, `80`, `53` in `crates/types/src/config.rs`), not destination hostnames, so a walled process can connect to arbitrary hosts on allowed ports.
- **Landlock filesystem rules:** Filesystem isolation is enforced via Bubblewrap mount namespaces (`handled_access_fs: 0` in Landlock).
- **Shared mutable `agent-home`:** Seats currently mount a shared `~/.local/share/factory/agent-home` directory read-write for CLI session state. Per-seat isolated agent homes are queued on the roadmap so builders and blind reviewers never share mutable CLI state.
- **Autonomous CLI execution:** Inside the wall, CLIs run with permission prompts bypassed (`--dangerously-skip-permissions`, `approval_policy=never`). Always scope `[projects.<name>]` mounts, `hide` lists, and `targets.toml` allowlists accordingly.

## Reporting a Vulnerability

Please do not open a public GitHub issue for security vulnerabilities. Report them privately through GitHub Security Advisories on `https://github.com/AndreiIlie/mork/security/advisories/new`. Include the affected file/path, reproduction steps, and kernel/bwrap version if sandbox-related.
