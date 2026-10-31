// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Status of a daemon launched by `z4j server start`.

use std::net::TcpStream;
use std::process::ExitCode;
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use zeta4j_transport_security::{client_tls_config_from_environment, validated_server_name};

use crate::ctl::utils::{
    bolt_handshake_over, check_bolt_handshake, check_health, check_http_ready, connect_probe,
    format_duration, get_log_path, get_pid_path, get_process_uptime, http_ready_over,
    is_process_running, read_pid_file, remove_pid_file,
};

use super::runtime_state::{
    RuntimeState, bolt_url, read_runtime_state, remove_runtime_state, runtime_state_path, web_url,
};

pub fn cmd_status(home: std::path::PathBuf) -> ExitCode {
    let pid = match read_pid_file(&home) {
        Some(pid) => pid,
        None => {
            if let Err(error) = remove_runtime_state(&home) {
                eprintln!("Error: {error}");
            }
            eprintln!("Zeta4j is not running");
            return ExitCode::FAILURE;
        }
    };

    if !is_process_running(pid) {
        if let Err(error) = remove_pid_file(&home) {
            eprintln!("Error: failed to remove stale PID file: {error}");
        }
        if let Err(error) = remove_runtime_state(&home) {
            eprintln!("Error: {error}");
        }
        eprintln!("Zeta4j is not running (stale process state removed)");
        return ExitCode::FAILURE;
    }

    let state = match read_runtime_state(&home) {
        Ok(state) if state.pid == pid => state,
        Ok(state) => {
            eprintln!(
                "Error: runtime state PID {} does not match running PID {pid}",
                state.pid
            );
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let tls = state
        .https_port
        .map(|_| TlsProbe::from_environment(&state.https_host));
    let skipped_reason = match &tls {
        Some(Err(reason)) => Some(reason.as_str()),
        _ => None,
    };
    let probes = probe_endpoints(&state, tls.as_ref().and_then(|tls| tls.as_ref().ok()));
    let uptime = get_process_uptime(pid);
    let (web_host, web_port) = probes.web_endpoint(&state);

    println!();
    println!("Zeta4j Server Status");
    println!("═══════════════════════════════════════════════════════");
    println!(
        "  Status:    RUNNING ({})",
        if probes.healthy() {
            "\x1b[32mOK\x1b[0m"
        } else {
            "\x1b[31mUNHEALTHY\x1b[0m"
        }
    );
    println!("  PID:       {pid}");
    if let Some(uptime_secs) = uptime {
        println!("  Uptime:    {}", format_duration(uptime_secs));
    }
    println!();
    println!("Endpoints:");
    println!(
        "  {:<10} {} ({})",
        if probes.tls { "HTTPS:" } else { "HTTP:" },
        web_url(probes.tls, web_host, web_port),
        probes.web.label()
    );
    println!(
        "  Bolt:      {} ({})",
        bolt_url(probes.tls, &state.bolt_host, state.bolt_port),
        probes.bolt.label()
    );
    println!(
        "  gRPC:      {}:{} ({})",
        state.grpc_host,
        state.grpc_port,
        if probes.grpc_tcp {
            "\x1b[32mTCP OK\x1b[0m"
        } else {
            "\x1b[31mFAIL\x1b[0m"
        }
    );
    if let Some(reason) = skipped_reason {
        println!("  Note:      TLS probes skipped ({reason}); showing TCP reachability");
    }

    println!();
    println!("Files:");
    println!("  PID File:   {}", get_pid_path(&home).display());
    println!("  State File: {}", runtime_state_path(&home).display());
    println!("  Log File:   {}", get_log_path(&home).display());
    println!();

    if probes.healthy() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Verified client TLS settings for probing a TLS-mode server.
pub(super) struct TlsProbe {
    config: Arc<ClientConfig>,
    server_name: ServerName<'static>,
}

impl TlsProbe {
    /// Uses the same trust references as every other client.
    pub(super) fn from_environment(default_server_name: &str) -> Result<Self, String> {
        let (config, server_name) =
            client_tls_config_from_environment(&[b"http/1.1"], default_server_name)
                .map_err(|error| error.to_string())?;
        let server_name = validated_server_name(&server_name).map_err(|error| error.to_string())?;
        Ok(Self {
            config: Arc::new(config),
            server_name,
        })
    }

    fn connect(&self, host: &str, port: u16) -> Option<StreamOwned<ClientConnection, TcpStream>> {
        let tcp = connect_probe(host, port)?;
        let connection =
            ClientConnection::new(Arc::clone(&self.config), self.server_name.clone()).ok()?;
        Some(StreamOwned::new(connection, tcp))
    }
}

/// Outcome of probing one endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    tcp: bool,
    /// `None` when the protocol probe could not run (TLS trust roots unavailable).
    protocol: Option<bool>,
}

impl Probe {
    fn healthy(self) -> bool {
        self.protocol.unwrap_or(self.tcp)
    }

    fn label(self) -> &'static str {
        match (self.protocol, self.tcp) {
            (Some(true), _) => "\x1b[32mOK\x1b[0m",
            (None, true) => "\x1b[33mTCP OK, TLS PROBE SKIPPED\x1b[0m",
            (Some(false), true) => "\x1b[33mTCP OK, PROTOCOL FAIL\x1b[0m",
            (_, false) => "\x1b[31mFAIL\x1b[0m",
        }
    }
}

#[derive(Debug)]
pub(super) struct EndpointProbes {
    tls: bool,
    web: Probe,
    bolt: Probe,
    grpc_tcp: bool,
}

impl EndpointProbes {
    pub(super) fn healthy(&self) -> bool {
        self.web.healthy() && self.bolt.healthy() && self.grpc_tcp
    }

    fn web_endpoint<'a>(&self, state: &'a RuntimeState) -> (&'a str, u16) {
        match state.https_port {
            Some(port) => (&state.https_host, port),
            None => (&state.http_host, state.http_port),
        }
    }
}

/// Probes every endpoint in the transport mode the runtime state records.
/// A TLS-mode server without usable client trust roots is judged on TCP
/// reachability instead of failing a plaintext probe against a TLS port.
pub(super) fn probe_endpoints(state: &RuntimeState, tls: Option<&TlsProbe>) -> EndpointProbes {
    let grpc_tcp = check_health(&state.grpc_host, state.grpc_port);
    let Some(https_port) = state.https_port else {
        return EndpointProbes {
            tls: false,
            web: Probe {
                tcp: check_health(&state.http_host, state.http_port),
                protocol: Some(check_http_ready(&state.http_host, state.http_port)),
            },
            bolt: Probe {
                tcp: check_health(&state.bolt_host, state.bolt_port),
                protocol: Some(check_bolt_handshake(&state.bolt_host, state.bolt_port)),
            },
            grpc_tcp,
        };
    };
    let web_host = state.https_host.as_str();
    EndpointProbes {
        tls: true,
        web: Probe {
            tcp: check_health(web_host, https_port),
            protocol: tls.map(|tls| {
                tls.connect(web_host, https_port)
                    .is_some_and(|mut stream| http_ready_over(&mut stream, web_host, https_port))
            }),
        },
        bolt: Probe {
            tcp: check_health(&state.bolt_host, state.bolt_port),
            protocol: tls.map(|tls| {
                tls.connect(&state.bolt_host, state.bolt_port)
                    .is_some_and(|mut stream| bolt_handshake_over(&mut stream))
            }),
        },
        grpc_tcp,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair, KeyUsagePurpose,
    };
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{RootCertStore, ServerConfig, ServerConnection};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn provider() -> Arc<rustls::crypto::CryptoProvider> {
        Arc::new(rustls::crypto::ring::default_provider())
    }

    /// TLS endpoint that answers `GET /health` and the Bolt handshake on one port.
    fn tls_server(
        connections: usize,
    ) -> Result<(u16, TlsProbe, JoinHandle<()>), Box<dyn std::error::Error>> {
        let mut issuer_params = CertificateParams::new(Vec::<String>::new())?;
        issuer_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        issuer_params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let issuer = CertifiedIssuer::self_signed(issuer_params, KeyPair::generate()?)?;
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec!["127.0.0.1".to_string()])?;
        params.is_ca = IsCa::ExplicitNoCa;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = params.signed_by(&key, &issuer)?;

        let server = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )?;
        let server = Arc::new(server);
        let mut roots = RootCertStore::empty();
        roots.add(issuer.der().clone())?;
        let client = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let probe = TlsProbe {
            config: Arc::new(client),
            server_name: validated_server_name("127.0.0.1")?,
        };

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(connections) {
                let Ok(tcp) = stream else { continue };
                let Ok(connection) = ServerConnection::new(Arc::clone(&server)) else {
                    continue;
                };
                let mut stream = StreamOwned::new(connection, tcp);
                let mut head = [0u8; 4];
                if stream.read_exact(&mut head).is_err() {
                    continue;
                }
                if head == [0x60, 0x60, 0xB0, 0x17] {
                    let mut proposals = [0u8; 16];
                    if stream.read_exact(&mut proposals).is_ok() {
                        let _ = stream.write_all(&[0, 0, 4, 4]);
                    }
                } else {
                    let mut request = [0u8; 512];
                    let _ = stream.read(&mut request);
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                }
                let _ = stream.flush();
                stream.conn.send_close_notify();
                let _ = stream.flush();
            }
        });
        Ok((port, probe, handle))
    }

    fn tls_state(port: u16) -> RuntimeState {
        RuntimeState {
            pid: 0,
            http_host: "127.0.0.1".to_string(),
            https_host: "127.0.0.1".to_string(),
            bolt_host: "127.0.0.1".to_string(),
            grpc_host: "127.0.0.1".to_string(),
            http_port: port,
            https_port: Some(port),
            bolt_port: port,
            grpc_port: port,
        }
    }

    #[test]
    fn tls_server_is_probed_over_tls() -> TestResult {
        // HTTPS probe, Bolt TLS probe and three TCP reachability checks.
        let (port, probe, server) = tls_server(5)?;
        let probes = probe_endpoints(&tls_state(port), Some(&probe));
        assert_eq!(probes.web.protocol, Some(true));
        assert_eq!(probes.bolt.protocol, Some(true));
        assert!(probes.healthy());
        server.join().map_err(|_| "TLS test server panicked")?;
        Ok(())
    }

    #[test]
    fn tls_server_without_trust_roots_reports_tcp_reachability() -> TestResult {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let probes = probe_endpoints(&tls_state(port), None);
        assert_eq!(probes.web.protocol, None);
        assert_eq!(probes.bolt.protocol, None);
        assert!(probes.web.tcp && probes.bolt.tcp);
        assert!(probes.healthy());
        Ok(())
    }
}
