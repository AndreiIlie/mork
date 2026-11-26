# Security Model & Reporting

Mork runs autonomous coding CLIs with interactive permission prompts disabled inside a Linux sandbox. The sandbox wall, private per-seat homes, symlink-safe host file operations, early diff secret scanning, and out-of-wall door are the security boundaries.

## What Mork Enforces Today

- **Fail-closed Linux sandbox (`crates/engine/src/jobs/wall.rs`, `wall_exec.rs`, `crates/sys/src/{landlock,seccomp}.rs`):**
  - Runs each walled turn under Bubblewrap with `--unshare-net`, `--unshare-user`, `--unshare-pid`, `--unshare-ipc`, `--cap-drop ALL`, and `--clearenv`.
  - Mounts the host root read-only (`--ro-bind / /`), covers host home directories, runtime dirs, and Mork's own state/sockets/keys with tmpfs, and exposes only the card's clone (writable for build seats, read-only for researchers and reviewers) and explicit `toolchains` / `hide` rules from config (including masking every configured `[projects.<name>.tickets] token` file).
  - Applies Landlock scoping for signals and abstract Unix sockets and installs a minimal seccomp-bpf filter (denying x32 ABI syscalls, `io_uring_setup`, `userfaultfd`, and `ptrace` with `EPERM` on both x86-64 and i386) before exec, failing closed if installation fails. Test-only wall overrides (`MORK_UNWALLED`, `MORK_TEST_*`) compile out when the `faults` feature is disabled.
- **Network egress for walled turns:** Each turn has private loopback for test servers. A host-side proxy accepts CONNECT for the exact host and port listed in `[mork] domains` or the card project's `domains`, checks the list before DNS, and relays opaque tunnel traffic. Host-local access requires a bridged port. The web bridge and configured SSH tunnel ports are excluded, including after a reload.
- **Private per-seat homes (`crates/engine/src/seat_homes.rs`, `login_files.rs`):**
  - Every seat receives its own isolated writable `HOME` directory pruned with its `io/` folder, preventing a builder or foreman from leaving mutable CLI state, hooks, or instructions visible to a blind reviewer.
  - CLI login files live in an engine-owned `logins/` directory outside every home, saved multi-account credentials live in `login-home/` (shown empty to every wall), and only explicitly listed `[mork] caches` / `cache_files` are shared across seats.
- **Symlink escape protection (`crates/sys/`):**
  - The engine never runs `git` using an agent-writable clone as its repository and accesses paths inside clones and seat homes only through `*_beneath` (`openat`-rooted) helpers so a symlink planted by a seat cannot redirect host writes.
- **Out-of-wall credentials, fail-closed hooks & early secret scanning (`crates/engine/src/door/`, `crates/engine/src/secrets.rs`, `crates/engine/src/cli/claude.rs`):**
  - Database, SSH, and API tokens stay outside the sandbox wall. Door operations run on the host side against `targets.toml` allowlists, require explicit operator approval for `misc.run`, and scrub secrets (including JSON-escaped forms) from logs and output.
  - Every project with configured checks scans candidate commits and the final diff for known secrets before verification and review.
  - Claude `PreToolUse` hooks configure `"onFailure": "block"` so hook failures fail closed, and unwalled Claude seats enforce protected-path blocks (`ACCESS.md`, `.ssh`) inside a fail-closed hook.
- **Local web bridge (`crates/engine/src/web/key.rs`, `crates/engine/src/web/http.rs`):**
  - Binds `127.0.0.1` only, authenticates requests with a 32-byte mode-`0600` key passed in the URL fragment using constant-time comparison, and enforces an `Origin` allowlist.

## Known Limitations (On the Public Roadmap)

- **Landlock filesystem rules:** Filesystem isolation is enforced via Bubblewrap mount namespaces (`handled_access_fs: 0` in Landlock).
- **Shared download caches (opt-in):** Any paths explicitly listed in `[mork] caches` / `cache_files` (such as `.cargo/registry`) are mounted read-write across seats; a seat can alter a cached source another seat later compiles.
- **Autonomous CLI execution:** Inside the wall, CLIs run with permission prompts bypassed (`--dangerously-skip-permissions`, `approval_policy=never`). Always scope `[projects.<name>]` mounts, `hide` lists, and `targets.toml` allowlists accordingly.

## Reporting a Vulnerability

Please do not open a public GitHub issue for security vulnerabilities. Report them privately through GitHub Security Advisories on `https://github.com/AndreiIlie/mork/security/advisories/new`. Include the affected file/path, reproduction steps, and kernel/bwrap version if sandbox-related.
