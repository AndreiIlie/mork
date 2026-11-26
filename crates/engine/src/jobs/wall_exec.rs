//! `mork wall-exec` (1.4.11.3) applies no-new-privileges, Landlock and seccomp before exec.
//! `--any-port` leaves TCP unrestricted (fac-93).

use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::faults;

/// The lowest Landlock ABI a full wall needs: abstract-socket and signal scoping.
pub const MIN_ABI: i64 = 6;

/// The kernel's Landlock ABI, or the test override (`MORK_TEST_LANDLOCK_ABI`, 1.10.6).
pub fn landlock_abi() -> i64 {
    if let Some(v) = faults::test_env("MORK_TEST_LANDLOCK_ABI").and_then(|v| v.parse().ok()) {
        return v;
    }
    mork_sys::landlock_abi().0
}

/// Why `wall-exec` didn't exec the job.
#[derive(Debug)]
pub enum WallExecError {
    /// No `--` before the command.
    NoSeparator,
    Bridge(std::io::Error),
    /// Nothing after `--`.
    NoCommand,
    /// A port that isn't a number from 0 to 65535.
    BadPort(String),
    /// `--partial` in a build without `faults`.
    PartialRefused,
    /// No-new-privileges couldn't be set.
    NoNewPrivs(std::io::Error),
    /// The kernel's Landlock ABI is too old for a full wall.
    AbiTooLow(i64),
    /// The Landlock domain couldn't be set.
    Landlock(std::io::Error),
    /// The syscall filter could not be installed.
    Seccomp(std::io::Error),
    /// The exec itself failed.
    Exec {
        program: String,
        err: std::io::Error,
    },
}

impl std::fmt::Display for WallExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bridge(e) => write!(f, "wall-exec: network bridge failed: {e}"),
            Self::NoSeparator => write!(f, "wall-exec: no `--` before the command"),
            Self::NoCommand => write!(f, "wall-exec: no command"),
            Self::BadPort(p) => write!(f, "wall-exec: bad port {p:?}"),
            Self::PartialRefused => write!(f, "wall-exec: --partial is a test setting, refused by this build"),
            Self::NoNewPrivs(e) => write!(f, "wall-exec: no-new-privileges failed: {e}"),
            Self::AbiTooLow(abi) => {
                write!(f, "wall-exec: Landlock ABI {abi} is below {MIN_ABI}; refusing to run unwalled")
            }
            Self::Landlock(e) => write!(f, "wall-exec: Landlock failed: {e}"),
            Self::Seccomp(e) => write!(f, "wall-exec: seccomp failed: {e}"),
            Self::Exec { program, err } => write!(f, "wall-exec: {program}: {err}"),
        }
    }
}

impl std::error::Error for WallExecError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub partial: bool,
    pub bridge: bool,
    /// `None`: `--any-port`.
    pub ports: Option<Vec<u16>>,
    pub argv: Vec<String>,
}

/// Parses the wall mode and the ports to bridge or permit before the command separator.
pub fn parse(args: &[String]) -> Result<Args, WallExecError> {
    let cut = args.iter().position(|a| a == "--").ok_or(WallExecError::NoSeparator)?;
    let (head, tail) = args.split_at(cut);
    let argv: Vec<String> = tail.iter().skip(1).cloned().collect();
    if argv.is_empty() {
        return Err(WallExecError::NoCommand);
    }
    let (mut partial, mut any) = (false, false);
    let mut ports = Vec::new();
    let mut bridge = false;
    for a in head {
        if a == "--partial" {
            partial = true;
        } else if a == "--bridge" {
            bridge = true;
        } else if a == "--any-port" {
            any = true;
        } else {
            ports.push(a.parse::<u16>().map_err(|_| WallExecError::BadPort(a.clone()))?);
        }
    }
    Ok(Args { partial, bridge, ports: (!any).then_some(ports), argv })
}

/// Sets the domain and execs. Returns only on failure, with the reason.
pub fn run(args: &[String]) -> WallExecError {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if a.partial && !faults::TEST_SETTINGS {
        return WallExecError::PartialRefused;
    }
    let Some((program, rest)) = a.argv.split_first() else { return WallExecError::NoCommand };
    let mut cmd = Command::new(program);
    if a.bridge
        && let Err(e) = bridge(&mut cmd, a.ports.as_deref().unwrap_or_default())
    {
        return WallExecError::Bridge(e);
    }
    if let Err(e) = mork_sys::set_no_new_privs() {
        return WallExecError::NoNewPrivs(e);
    }
    let abi = landlock_abi();
    if abi < MIN_ABI && !a.partial {
        return WallExecError::AbiTooLow(abi);
    }
    let restriction =
        mork_sys::Restriction { tcp_connect: a.ports.filter(|_| abi >= 4 && !a.bridge), scope: abi >= MIN_ABI };
    if let Err(e) = mork_sys::restrict_self(&restriction) {
        return WallExecError::Landlock(e);
    }
    if let Err(e) = mork_sys::install_wall_filter() {
        return WallExecError::Seccomp(e);
    }
    // bwrap's `--chdir` sets `PWD`; the wall's environment is only what Mork sets
    // (1.4.11.4).
    let err = cmd.args(rest).env_remove("PWD").exec();
    WallExecError::Exec { program: program.clone(), err }
}

fn bridge(cmd: &mut Command, ports: &[u16]) -> std::io::Result<()> {
    use std::io::Read;
    use std::net::{Ipv4Addr, Shutdown, TcpListener};
    let mut pair = mork_sys::wall_bridge::inherited_bridge()?;
    for port in ports {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, *port))?;
        mork_sys::wall_bridge::send_listener(&pair, &listener, 1)?;
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let url = format!("http://{}", listener.local_addr()?);
    mork_sys::wall_bridge::send_listener(&pair, &listener, 0)?;
    pair.shutdown(Shutdown::Write)?;
    let mut ready = [0];
    pair.read_exact(&mut ready)?;
    if ready != [1] {
        return Err(std::io::Error::other("wall proxy did not accept its listeners"));
    }
    for name in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
        cmd.env(name, &url);
    }
    for name in ["NO_PROXY", "no_proxy"] {
        cmd.env(name, "localhost,127.0.0.1,::1,.localhost");
    }
    Ok(())
}
