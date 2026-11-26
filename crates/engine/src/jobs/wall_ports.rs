use std::fs::{File, OpenOptions};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use mork_types::config::Config;

use crate::paths::Home;

pub(super) const FILE: &str = "wall-service-ports.json";

pub(crate) fn start(home: &Home, config: &Config) -> io::Result<Option<u16>> {
    // Surviving proxies lock this same inode through connect, including while an ephemeral port is assigned.
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(home.root().join(FILE))?;
    file.lock()?;
    file.set_len(0)?;
    let assets = config.mork.web_assets.as_ref().map(std::path::PathBuf::from);
    let web = config.mork.web_port.and_then(|port| crate::web::start(home, port, assets)).map(|addr| addr.port());
    let ports: Vec<_> = config.tunnels.values().map(|t| t.port).chain(web).collect();
    serde_json::to_writer(&file, &ports)?;
    Ok(web)
}

pub(super) fn connect(file: &Path, addr: SocketAddr) -> io::Result<Option<TcpStream>> {
    let Some(_guard) = permit(file, addr.port())? else { return Ok(None) };
    TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map(Some)
}

fn permit(file: &Path, port: u16) -> io::Result<Option<File>> {
    let file = File::open(file)?;
    file.lock_shared()?;
    let ports: Vec<u16> = serde_json::from_reader(&file)?;
    Ok((!ports.contains(&port)).then_some(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_ports_follow_new_service_reservations() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = mork_fakes::TestHome::new();
        let home = Home::new(tmp.mork_home());
        let file = home.root().join(FILE);
        let base = "[mork]\n[git]\nauthor = 'Test <a@b>'";
        let config = crate::config::parse(base)?;
        start(&home, &config)?;
        let guard = permit(&file, 4321)?.ok_or("fixture port unavailable")?;
        let writer = File::options().write(true).open(&file)?;
        assert!(matches!(writer.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        drop(guard);
        let next = format!("{base}\n[tunnels.web]\nalias='host'\nport=4321\nremote_port=80\nurl='http://localhost'");
        start(&home, &crate::config::parse(&next)?)?;
        assert!(permit(&file, 4321)?.is_none());
        assert!(permit(&file, 4322)?.is_some());
        Ok(())
    }
}
