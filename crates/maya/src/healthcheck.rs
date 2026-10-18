//! `maya healthcheck`: imagen saknar curl, så Dockers hälsokontroll anropar binären själv.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

pub fn run() -> ExitCode {
    let mut addr = super::listen_addr();
    if addr.ip().is_unspecified() {
        addr = match addr {
            SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::LOCALHOST, addr.port())),
            SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::LOCALHOST, addr.port())),
        };
    }
    match get_healthz(addr) {
        Ok(status) if status.split_whitespace().nth(1) == Some("200") => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("Maya svarar inte som väntat på /healthz: {status}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("Maya svarar inte på {addr}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn get_healthz(addr: SocketAddr) -> std::io::Result<String> {
    let timeout = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    write!(
        stream,
        "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response.lines().next().unwrap_or_default().to_string())
}
