use std::io;
use std::mem::offset_of;

use libc::{seccomp_data, sock_filter, sock_fprog};

const fn instruction(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
    sock_filter { code: code as u16, jt, jf, k }
}

const fn equal(value: u32, yes: u8, no: u8) -> sock_filter {
    instruction(libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K, value, yes, no)
}

const LOAD_WORD: u32 = libc::BPF_LD | libc::BPF_W | libc::BPF_ABS;
const LOAD_ARCH: sock_filter = instruction(LOAD_WORD, offset_of!(seccomp_data, arch) as u32, 0, 0);
const LOAD_NR: sock_filter = instruction(LOAD_WORD, offset_of!(seccomp_data, nr) as u32, 0, 0);
const ALLOW: sock_filter = instruction(libc::BPF_RET | libc::BPF_K, libc::SECCOMP_RET_ALLOW, 0, 0);
const DENY: sock_filter = instruction(libc::BPF_RET | libc::BPF_K, libc::SECCOMP_RET_ERRNO | libc::EPERM as u32, 0, 0);

// Both syscall tables end in ALLOW, DENY. A match skips directly to DENY.
const fn blocked_calls(uring: u32, userfaultfd: u32, ptrace: u32) -> [sock_filter; 3] {
    [equal(uring, 3, 0), equal(userfaultfd, 2, 0), equal(ptrace, 1, 0)]
}

const NATIVE: [sock_filter; 3] =
    blocked_calls(libc::SYS_io_uring_setup as u32, libc::SYS_userfaultfd as u32, libc::SYS_ptrace as u32);

#[cfg(target_arch = "x86_64")]
const FILTER: &[sock_filter] = &{
    const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
    const AUDIT_ARCH_I386: u32 = 0x4000_0003;
    const X32_BIT: u32 = 1 << 30;
    // asm/unistd_32.h: the compat numbers belong to the calling ABI, even in a 64-bit process.
    let [uring32, uffd32, ptrace32] = blocked_calls(425, 374, 26);
    let [uring, uffd, ptrace] = NATIVE;
    [
        LOAD_ARCH,
        equal(AUDIT_ARCH_X86_64, 0, 7),
        LOAD_NR,
        instruction(libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K, X32_BIT, 4, 0),
        uring,
        uffd,
        ptrace,
        ALLOW,
        DENY,
        equal(AUDIT_ARCH_I386, 1, 0),
        DENY,
        LOAD_NR,
        uring32,
        uffd32,
        ptrace32,
        ALLOW,
        DENY,
    ]
};

#[cfg(not(target_arch = "x86_64"))]
fn native_audit_arch() -> io::Result<u32> {
    // linux/audit.h identifies syscall conventions independently of the executable's ELF class.
    let little = if cfg!(target_endian = "little") { 0x4000_0000 } else { 0 };
    let arch = match std::env::consts::ARCH {
        "x86" => 0x4000_0003,
        "arm" => 0x28 | little,
        "aarch64" => 0xc000_00b7,
        "riscv32" => 0x4000_00f3,
        "riscv64" => 0xc000_00f3,
        "powerpc" => 0x14,
        "powerpc64" => 0x8000_0015 | little,
        "s390x" => 0x8000_0016,
        "sparc" => 0x02,
        "sparc64" => 0x8000_002b,
        "mips" | "mips32r6" => 0x08 | little,
        "mips64" | "mips64r6" => 0x8000_0008 | little,
        "loongarch64" => 0xc000_0102,
        _ => return Err(io::Error::new(io::ErrorKind::Unsupported, "unknown syscall architecture")),
    };
    Ok(arch)
}

/// Installs the wall's inherited syscall filter on the calling thread before exec.
pub fn install_wall_filter() -> io::Result<()> {
    #[cfg(target_arch = "x86_64")]
    let filter = FILTER;
    #[cfg(not(target_arch = "x86_64"))]
    let filter = {
        let [uring, uffd, ptrace] = NATIVE;
        [LOAD_ARCH, equal(native_audit_arch()?, 1, 0), DENY, LOAD_NR, uring, uffd, ptrace, ALLOW, DENY]
    };
    let program = sock_fprog { len: filter.len() as u16, filter: filter.as_ptr().cast_mut() };
    // SAFETY: the kernel reads the live instruction array and copies it before this call returns.
    let result = unsafe { libc::syscall(libc::SYS_seccomp, libc::SECCOMP_SET_MODE_FILTER, 0u32, &raw const program) };
    if result == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
