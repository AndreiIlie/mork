// 1.4.11.3: the domain `factory wall-exec` sets before it execs a job.

use std::io;

const SYS_CREATE_RULESET: libc::c_long = 444;
const SYS_ADD_RULE: libc::c_long = 445;
const SYS_RESTRICT_SELF: libc::c_long = 446;
const CREATE_RULESET_VERSION: u32 = 1;
const RULE_NET_PORT: libc::c_int = 2;
const ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;
const SCOPE_ABSTRACT_UNIX_SOCKET: u64 = 1 << 0;
const SCOPE_SIGNAL: u64 = 1 << 1;

/// The kernel's Landlock ABI version; 0 when Landlock is off or absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LandlockAbi(pub i64);

pub fn abi() -> LandlockAbi {
    // SAFETY: with a null attribute and the version flag, the call only returns a number.
    let r = unsafe { libc::syscall(SYS_CREATE_RULESET, std::ptr::null::<u8>(), 0usize, CREATE_RULESET_VERSION) };
    LandlockAbi(if r < 0 { 0 } else { r })
}

/// What the domain restricts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restriction {
    /// TCP ports a connect is allowed to; `None` leaves TCP unrestricted.
    pub tcp_connect: Option<Vec<u16>>,
    /// Scope abstract unix sockets and signals (ABI 6).
    pub scope: bool,
}

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

#[repr(C, packed)]
struct NetPortAttr {
    allowed_access: u64,
    port: u64,
}

fn check(r: libc::c_long) -> io::Result<libc::c_long> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}

/// Restricts the calling thread, and what it execs, to `what`. No-new-privileges must be set.
pub fn restrict_self(what: &Restriction) -> io::Result<()> {
    let attr = RulesetAttr {
        handled_access_fs: 0,
        handled_access_net: if what.tcp_connect.is_some() { ACCESS_NET_CONNECT_TCP } else { 0 },
        scoped: if what.scope { SCOPE_ABSTRACT_UNIX_SOCKET | SCOPE_SIGNAL } else { 0 },
    };
    if attr.handled_access_net == 0 && attr.scoped == 0 {
        return Ok(());
    }
    // SAFETY: `attr` is a live repr(C) struct of the size passed.
    let fd =
        check(unsafe { libc::syscall(SYS_CREATE_RULESET, &raw const attr, std::mem::size_of::<RulesetAttr>(), 0u32) })?;
    let fd = libc::c_int::try_from(fd).map_err(|_| io::Error::other("ruleset fd out of range"))?;
    let result = add_ports_and_restrict(fd, what.tcp_connect.as_deref().unwrap_or(&[]));
    // SAFETY: closing the ruleset descriptor we created.
    unsafe { libc::close(fd) };
    result
}

fn add_ports_and_restrict(fd: libc::c_int, ports: &[u16]) -> io::Result<()> {
    for &port in ports {
        let rule = NetPortAttr { allowed_access: ACCESS_NET_CONNECT_TCP, port: u64::from(port) };
        // SAFETY: `rule` is a live packed struct as the kernel expects for a net-port rule.
        check(unsafe { libc::syscall(SYS_ADD_RULE, fd, RULE_NET_PORT, &raw const rule, 0u32) })?;
    }
    // SAFETY: restrict_self takes the ruleset descriptor and flags.
    check(unsafe { libc::syscall(SYS_RESTRICT_SELF, fd, 0u32) })?;
    Ok(())
}
