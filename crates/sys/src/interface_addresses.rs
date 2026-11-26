use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Returns the host's current IPv4 and IPv6 interface addresses for local-destination checks.
pub fn interface_addresses() -> io::Result<Vec<IpAddr>> {
    let mut list = std::ptr::null_mut();
    // SAFETY: on success getifaddrs gives a linked list valid until freeifaddrs below.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut addresses = Vec::new();
    let mut next = list;
    while !next.is_null() {
        // SAFETY: next is a node in the live list. The address family determines its layout.
        unsafe {
            let addr = (*next).ifa_addr;
            next = (*next).ifa_next;
            if addr.is_null() {
                continue;
            }
            match i32::from((*addr).sa_family) {
                libc::AF_INET => {
                    let raw = (*addr.cast::<libc::sockaddr_in>()).sin_addr.s_addr;
                    addresses.push(Ipv4Addr::from(raw.to_ne_bytes()).into());
                }
                libc::AF_INET6 => {
                    let raw = (*addr.cast::<libc::sockaddr_in6>()).sin6_addr.s6_addr;
                    addresses.push(IpAddr::V6(Ipv6Addr::from(raw)).to_canonical());
                }
                _ => {}
            }
        }
    }
    // SAFETY: list is the pointer returned by the successful getifaddrs call.
    unsafe { libc::freeifaddrs(list) };
    Ok(addresses)
}
