// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    io::{self, Read as _, Write as _},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    str,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use url::{Host, Url};

use super::super::security::EgressGuard;

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 64;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(super) struct EgressProxy {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl EgressProxy {
    pub(super) fn start(
        guard: EgressGuard,
        denied: Arc<Mutex<Option<String>>>,
        deadline: Instant,
    ) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind Browser egress proxy")?;
        listener
            .set_nonblocking(true)
            .context("configure Browser egress proxy")?;
        let address = listener.local_addr()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let listener_stopped = stopped.clone();
        let listener = thread::Builder::new()
            .name("molesignal-browser-egress".into())
            .spawn(move || {
                accept_connections(listener, guard, denied, deadline, &listener_stopped);
            })
            .context("start Browser egress proxy")?;
        Ok(Self {
            address,
            stopped,
            listener: Some(listener),
        })
    }

    pub(super) fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn accept_connections(
    listener: TcpListener,
    guard: EgressGuard,
    denied: Arc<Mutex<Option<String>>>,
    deadline: Instant,
    stopped: &AtomicBool,
) {
    let active = Arc::new(AtomicUsize::new(0));
    while !stopped.load(Ordering::Acquire) && Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::AcqRel);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let guard = guard.clone();
                let denied = denied.clone();
                let connection_active = active.clone();
                let spawned = thread::Builder::new()
                    .name("molesignal-browser-proxy-connection".into())
                    .spawn(move || {
                        let _ = handle_connection(stream, &guard, &denied, deadline);
                        connection_active.fetch_sub(1, Ordering::AcqRel);
                    });
                if spawned.is_err() {
                    active.fetch_sub(1, Ordering::AcqRel);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(
    mut client: TcpStream,
    guard: &EgressGuard,
    denied: &Mutex<Option<String>>,
    deadline: Instant,
) -> Result<()> {
    configure_stream(&client, deadline)?;
    let (request, header_end) = read_request_head(&mut client, deadline)?;
    let header =
        str::from_utf8(&request[..header_end]).context("Browser proxy request is not UTF-8")?;
    let first_line_end = header
        .find("\r\n")
        .ok_or_else(|| anyhow!("Browser proxy request has no request line"))?;
    let mut request_line = header[..first_line_end].split_whitespace();
    let method = request_line
        .next()
        .ok_or_else(|| anyhow!("Browser proxy request has no method"))?;
    let target = request_line
        .next()
        .ok_or_else(|| anyhow!("Browser proxy request has no target"))?;
    let version = request_line
        .next()
        .ok_or_else(|| anyhow!("Browser proxy request has no version"))?;
    if request_line.next().is_some() {
        bail!("Browser proxy request line is invalid");
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = parse_authority(target)?;
        let mut upstream = connect_target(guard, denied, &host, port, deadline)?;
        configure_stream(&upstream, deadline)?;
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        if request.len() > header_end {
            upstream.write_all(&request[header_end..])?;
        }
        return tunnel(client, upstream);
    }

    let url = Url::parse(target).context("parse Browser proxy request target")?;
    if url.scheme() != "http" {
        bail!("Browser proxy only accepts HTTP requests or CONNECT tunnels");
    }
    let host = normalized_host(&url)?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("Browser proxy request has no port"))?;
    let mut upstream = connect_target(guard, denied, &host, port, deadline)?;
    configure_stream(&upstream, deadline)?;
    let rewritten = rewrite_http_request(method, version, &url, header, &request[header_end..]);
    upstream.write_all(&rewritten)?;
    tunnel(client, upstream)
}

fn connect_target(
    guard: &EgressGuard,
    denied: &Mutex<Option<String>>,
    host: &str,
    port: u16,
    deadline: Instant,
) -> Result<TcpStream> {
    let addresses = guard.resolve_blocking(host, port).map_err(|error| {
        record_denied(
            denied,
            format!("Browser egress policy rejected {host}:{port}: {error}"),
        );
        error
    })?;
    let mut last_error = None;
    for address in addresses {
        let timeout = remaining(deadline)?.min(Duration::from_secs(10));
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.map_or_else(
        || anyhow!("Browser target resolved to no approved addresses"),
        anyhow::Error::from,
    ))
}

fn read_request_head(stream: &mut TcpStream, deadline: Instant) -> Result<(Vec<u8>, usize)> {
    let mut request = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok((request, position + 4));
        }
        if request.len() >= MAX_HEADER_BYTES {
            bail!("Browser proxy request headers exceed 64 KiB");
        }
        stream.set_read_timeout(Some(remaining(deadline)?.min(Duration::from_secs(2))))?;
        match stream.read(&mut chunk) {
            Ok(0) => bail!("Browser proxy connection closed before request headers"),
            Ok(read) => request.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn rewrite_http_request(
    method: &str,
    version: &str,
    url: &Url,
    header: &str,
    buffered_body: &[u8],
) -> Vec<u8> {
    let mut origin = url.path().to_string();
    if origin.is_empty() {
        origin.push('/');
    }
    if let Some(query) = url.query() {
        origin.push('?');
        origin.push_str(query);
    }
    let upgrade = header
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("upgrade:"));
    let mut rewritten = format!("{method} {origin} {version}\r\n").into_bytes();
    for line in header.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:")
            || lower.starts_with("proxy-authorization:")
            || (!upgrade && lower.starts_with("connection:"))
        {
            continue;
        }
        rewritten.extend_from_slice(line.as_bytes());
        rewritten.extend_from_slice(b"\r\n");
    }
    if !upgrade {
        rewritten.extend_from_slice(b"Connection: close\r\n");
    }
    rewritten.extend_from_slice(b"\r\n");
    rewritten.extend_from_slice(buffered_body);
    rewritten
}

fn parse_authority(authority: &str) -> Result<(String, u16)> {
    let url =
        Url::parse(&format!("https://{authority}/")).context("parse Browser CONNECT authority")?;
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() {
        bail!("Browser CONNECT authority is invalid");
    }
    Ok((normalized_host(&url)?, url.port().unwrap_or(443)))
}

fn normalized_host(url: &Url) -> Result<String> {
    match url.host() {
        Some(Host::Domain(host)) => Ok(host.to_string()),
        Some(Host::Ipv4(host)) => Ok(host.to_string()),
        Some(Host::Ipv6(host)) => Ok(host.to_string()),
        None => Err(anyhow!("Browser proxy request has no host")),
    }
}

fn configure_stream(stream: &TcpStream, deadline: Instant) -> Result<()> {
    let timeout = remaining(deadline)?.min(Duration::from_secs(30));
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| anyhow!("Browser Journey timed out"))
}

fn tunnel(mut client: TcpStream, mut upstream: TcpStream) -> Result<()> {
    let mut client_reader = client.try_clone()?;
    let mut upstream_writer = upstream.try_clone()?;
    let forward = thread::spawn(move || {
        let result = io::copy(&mut client_reader, &mut upstream_writer);
        let _ = upstream_writer.shutdown(Shutdown::Both);
        result
    });
    let reverse = io::copy(&mut upstream, &mut client);
    let _ = client.shutdown(Shutdown::Both);
    let _ = upstream.shutdown(Shutdown::Both);
    let _ = forward.join();
    reverse.map(|_| ()).map_err(Into::into)
}

fn record_denied(denied: &Mutex<Option<String>>, message: String) {
    if let Ok(mut current) = denied.lock()
        && current.is_none()
    {
        *current = Some(message);
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{parse_authority, rewrite_http_request};

    #[test]
    fn parses_connect_authorities() {
        assert_eq!(
            parse_authority("example.test:8443").expect("authority"),
            ("example.test".into(), 8443)
        );
        assert_eq!(
            parse_authority("[2001:db8::1]:443").expect("IPv6 authority"),
            ("2001:db8::1".into(), 443)
        );
    }

    #[test]
    fn rewrites_absolute_http_requests() {
        let url = Url::parse("http://example.test/path?q=1").expect("URL");
        let request = rewrite_http_request(
            "GET",
            "HTTP/1.1",
            &url,
            "GET http://example.test/path?q=1 HTTP/1.1\r\nHost: example.test\r\nProxy-Connection: keep-alive\r\n\r\n",
            &[],
        );
        let request = String::from_utf8(request).expect("UTF-8 request");
        assert!(request.starts_with("GET /path?q=1 HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.test\r\n"));
        assert!(request.contains("Connection: close\r\n"));
        assert!(!request.contains("Proxy-Connection"));
    }
}
