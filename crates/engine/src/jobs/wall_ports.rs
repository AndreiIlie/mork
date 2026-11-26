use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use mork_types::config::Config;

use crate::paths::Home;

pub(super) const FILE: &str = "wall-service-ports.json";
const TUNNELS: &str = "wall-tunnel-ports.lock";

pub(crate) fn start(home: &Home, config: &Config) -> io::Result<BTreeSet<u16>> {
    // Surviving proxies lock this same inode through connect, including while an ephemeral port is assigned.
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(home.root().join(FILE))?;
    file.lock()?;
    OpenOptions::new().create(true).append(true).open(home.root().join(TUNNELS))?;
    file.set_len(0)?;
    let assets = config.mork.web_assets.as_ref().map(std::path::PathBuf::from);
    let web = config.mork.web_port.and_then(|port| crate::web::start(home, port, assets)).map(|addr| addr.port());
    let ports: BTreeSet<_> = config.tunnels.values().map(|t| t.port).chain(web).collect();
    serde_json::to_writer(&file, &ports)?;
    Ok(ports)
}

pub(super) fn connect(file: &Path, addr: SocketAddr) -> io::Result<Option<TcpStream>> {
    let Some(_guard) = permit(file, addr.port())? else { return Ok(None) };
    TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map(Some)
}

fn permit(file: &Path, port: u16) -> io::Result<Option<File>> {
    let (guard, ports) = locked_ports(file)?;
    // A removed tunnel may still own its port. New tunnels can only open on a currently reserved port.
    let free = !ports.contains(&port) && mork_sys::byte_is_free(&File::open(file.with_file_name(TUNNELS))?, port)?;
    Ok(free.then_some(guard))
}

pub(super) fn tunnel_input(home: &Home, port: u16) -> io::Result<File> {
    let (_guard, ports) = locked_ports(&home.root().join(FILE))?;
    if !ports.contains(&port) {
        return Err(io::Error::other(format!("local port {port} is not reserved for the tunnel")));
    }
    // SSH and its guard inherit only this port's lock, so neither can hold up publication during reload.
    let input = File::open(home.root().join(TUNNELS))?;
    mork_sys::hold_byte(&input, port)?;
    Ok(input)
}

fn locked_ports(path: &Path) -> io::Result<(File, BTreeSet<u16>)> {
    let file = File::open(path)?;
    file.lock_shared()?;
    let ports = serde_json::from_reader(&file)?;
    Ok((file, ports))
}
