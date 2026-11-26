use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use mork_types::job::Net;

use super::folder::JobDir;
use crate::config_checks::destination;

/// Runs the host side of one walled turn's proxy until its supervisor ends the job.
pub fn main(folder: &Path) -> i32 {
    match run(folder) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("wall-proxy: {e}");
            1
        }
    }
}

fn run(folder: &Path) -> io::Result<()> {
    let net = JobDir::new(folder).spec()?.and_then(|s| s.net).ok_or_else(|| io::Error::other("no network policy"))?;
    let net = Arc::new(net);
    let mut pair = UnixStream::from(io::stdin().as_fd().try_clone_to_owned()?);
    let mut threads = Vec::new();
    while let Some((label, listener)) = mork_sys::wall_bridge::recv_listener(pair.as_fd())? {
        let port = listener.local_addr()?.port();
        let forward = match label {
            0 => None,
            1 if net.host_ports.contains(&port) => Some(port),
            _ => return Err(io::Error::other("unexpected wall listener")),
        };
        let net = Arc::clone(&net);
        threads.push(thread::Builder::new().spawn(move || serve(listener, forward, &net))?);
    }
    pair.write_all(&[1])?;
    drop(pair);
    for handle in threads {
        handle.join().map_err(|_| io::Error::other("wall listener stopped"))??;
    }
    Ok(())
}

fn serve(listener: TcpListener, forward: Option<u16>, net: &Arc<Net>) -> io::Result<()> {
    for stream in listener.incoming() {
        let stream = stream?;
        let net = Arc::clone(net);
        thread::Builder::new().spawn(move || match forward {
            Some(port) => {
                if let Ok(peer) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                    let _ = relay(stream, peer);
                }
            }
            None => {
                let _ = tunnel(stream, &net);
            }
        })?;
    }
    Ok(())
}

fn allowed<'a>(head: &'a [u8], entries: &[String]) -> Result<(&'a str, u16), String> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    if !matches!(req.parse(head), Ok(httparse::Status::Complete(_))) || req.method != Some("CONNECT") {
        return Err("Only CONNECT to destinations listed in mork.toml domains is allowed".into());
    }
    let target = req.path.unwrap_or_default();
    let denied = || format!("Destination {target} is refused by mork.toml domains");
    let (host, port) = destination(target, None).ok_or_else(denied)?;
    let listed = entries
        .iter()
        .filter_map(|entry| destination(entry, Some(443)))
        .any(|(name, p)| p == port && name.eq_ignore_ascii_case(host));
    if listed { Ok((host, port)) } else { Err(denied()) }
}

fn address_allowed(addr: SocketAddr, own: &[IpAddr], ports: &[u16]) -> bool {
    let ip = addr.ip().to_canonical();
    let local = ip.is_loopback() || ip.is_unspecified() || own.contains(&ip);
    !local || ports.contains(&addr.port())
}

enum Refused {
    Policy(String),
    Unavailable(io::Error),
}

fn connect(
    head: &[u8],
    net: &Net,
    resolve: impl FnOnce(&str, u16) -> io::Result<Vec<SocketAddr>>,
) -> Result<TcpStream, Refused> {
    let (host, port) = allowed(head, &net.domains).map_err(Refused::Policy)?;
    let addresses = resolve(host, port).map_err(Refused::Unavailable)?;
    let own = mork_sys::interface_addresses().map_err(Refused::Unavailable)?;
    let mut failed = None;
    for addr in addresses.into_iter().filter(|a| address_allowed(*a, &own, &net.host_ports)) {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
            Ok(s) => return Ok(s),
            Err(e) => failed = Some(e),
        }
    }
    Err(failed.map_or_else(
        || Refused::Policy(format!("Local destination {host}:{port} is refused by mork.toml host_ports")),
        Refused::Unavailable,
    ))
}

fn tunnel(mut client: TcpStream, net: &Net) -> io::Result<()> {
    let Some((head, extra)) = crate::web::http::read_head_bytes(&mut client)? else { return Ok(()) };
    client.set_read_timeout(None)?;
    let mut peer = match connect(&head, net, |host, port| Ok((host, port).to_socket_addrs()?.collect())) {
        Ok(s) => s,
        Err(e) => {
            let (status, reason) = match e {
                Refused::Policy(text) => ("403 Forbidden", text),
                Refused::Unavailable(e) => ("502 Bad Gateway", e.to_string()),
            };
            return write!(
                client,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reason}",
                reason.len()
            );
        }
    };
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    peer.write_all(&extra)?;
    relay(client, peer)
}

fn relay(a: TcpStream, b: TcpStream) -> io::Result<()> {
    let (a2, b2) = (a.try_clone()?, b.try_clone()?);
    let worker = thread::Builder::new().spawn(move || copy(a, b))?;
    copy(b2, a2);
    worker.join().map_err(|_| io::Error::other("relay stopped"))
}

fn copy(mut from: TcpStream, mut to: TcpStream) {
    let how = if io::copy(&mut from, &mut to).is_ok() { Shutdown::Write } else { Shutdown::Both };
    let _ = to.shutdown(how);
    if how == Shutdown::Both {
        let _ = from.shutdown(how);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_requests_skip_dns() {
        let net = Net { domains: vec!["registry.example".into(), "build.example:8443".into()], host_ports: vec![] };
        for req in [
            "GET http://registry.example/ HTTP/1.1",
            "CONNECT bad.example:443 HTTP/1.1",
            "CONNECT registry.example:8443 HTTP/1.1",
            "CONNECT registry.example HTTP/1.1",
        ] {
            let head = format!("{req}\r\n\r\n");
            assert!(matches!(
                connect(head.as_bytes(), &net, |_, _| Err(io::Error::other("resolved denied request"))),
                Err(Refused::Policy(_))
            ));
        }
        assert_eq!(
            allowed(b"CONNECT REGISTRY.EXAMPLE:443 HTTP/1.1\r\n\r\n", &net.domains),
            Ok(("REGISTRY.EXAMPLE", 443))
        );
        assert_eq!(allowed(b"CONNECT build.example:8443 HTTP/1.1\r\n\r\n", &net.domains), Ok(("build.example", 8443)));
    }

    #[test]
    fn machine_addresses_require_a_bridged_port() -> Result<(), std::net::AddrParseError> {
        let own = [IpAddr::from([192, 0, 2, 1])];
        for addr in ["[::ffff:127.0.0.1]:443", "[::1]:443", "0.0.0.0:443", "192.0.2.1:443"] {
            let addr = addr.parse()?;
            assert!(!address_allowed(addr, &own, &[]));
            assert!(address_allowed(addr, &own, &[443]));
        }
        assert!(address_allowed("192.0.2.2:443".parse()?, &own, &[]));
        Ok(())
    }
}
