// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Local server protocol health-check utilities.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// 상세 헬스 상태
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// TCP 연결 (HTTP 포트)
    pub tcp_http: bool,
    /// TCP 연결 (Bolt 포트)
    pub tcp_bolt: bool,
    /// HTTP 프로토콜 준비 상태
    pub http_ready: bool,
    /// Bolt 프로토콜 핸드셰이크
    pub bolt_handshake: bool,
}

impl HealthStatus {
    /// 전체 상태
    pub fn overall(&self) -> &'static str {
        if self.http_ready && self.bolt_handshake {
            "OK"
        } else if self.tcp_http || self.tcp_bolt {
            "DEGRADED"
        } else {
            "FAIL"
        }
    }

    /// 컬러 출력용 상태
    pub fn overall_colored(&self) -> String {
        match self.overall() {
            "OK" => "\x1b[32mOK\x1b[0m".to_string(),             // 녹색
            "DEGRADED" => "\x1b[33mDEGRADED\x1b[0m".to_string(), // 황색
            "FAIL" => "\x1b[31mFAIL\x1b[0m".to_string(),         // 적색
            s => s.to_string(),
        }
    }
}

/// 간단한 TCP 연결 확인
pub fn check_health(host: &str, port: u16) -> bool {
    let addr = match format!("{}:{}", host, port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok()
}

/// 프로토콜 수준 헬스체크
pub fn check_health_detailed(host: &str, http_port: u16, bolt_port: u16) -> HealthStatus {
    HealthStatus {
        tcp_http: check_health(host, http_port),
        tcp_bolt: check_health(host, bolt_port),
        http_ready: check_http_ready(host, http_port),
        bolt_handshake: check_bolt_handshake(host, bolt_port),
    }
}

/// Opens a probe connection with bounded connect, read and write timeouts.
pub fn connect_probe(host: &str, port: u16) -> Option<TcpStream> {
    let addr = format!("{}:{}", host, port).parse().ok()?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    Some(stream)
}

/// HTTP 프로토콜 생존 확인
/// 인증과 부트스트랩 준비 상태에 독립적인 GET /health 엔드포인트를 체크한다.
pub fn check_http_ready(host: &str, port: u16) -> bool {
    connect_probe(host, port).is_some_and(|mut stream| http_ready_over(&mut stream, host, port))
}

/// Sends `GET /health` over an established (plaintext or TLS) stream.
pub fn http_ready_over(stream: &mut (impl Read + Write), host: &str, port: u16) -> bool {
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
        host, port
    );
    if stream.write_all(request.as_bytes()).is_err() || stream.flush().is_err() {
        return false;
    }
    let mut response = vec![0u8; 1024];
    match stream.read(&mut response) {
        Ok(n) if n > 0 => {
            let response_str = String::from_utf8_lossy(&response[..n]);
            response_str.starts_with("HTTP/1.") && response_str.contains(" 200 ")
        }
        _ => false,
    }
}

/// Bolt 프로토콜 핸드셰이크 확인
/// Bolt 매직 넘버와 버전 협상 체크
pub fn check_bolt_handshake(host: &str, port: u16) -> bool {
    connect_probe(host, port).is_some_and(|mut stream| bolt_handshake_over(&mut stream))
}

/// Performs the Bolt magic and version negotiation over an established stream.
pub fn bolt_handshake_over(stream: &mut (impl Read + Write)) -> bool {
    // Bolt 핸드셰이크: 매직 넘버 (0x6060B017) + 버전 제안 (4개)
    let handshake: [u8; 20] = [
        0x60, 0x60, 0xB0, 0x17, // 매직 넘버
        0x00, 0x00, 0x04, 0x04, // Bolt 4.4
        0x00, 0x00, 0x04, 0x03, // Bolt 4.3
        0x00, 0x00, 0x04, 0x02, // Bolt 4.2
        0x00, 0x00, 0x00, 0x00, // 미사용
    ];
    if stream.write_all(&handshake).is_err() || stream.flush().is_err() {
        return false;
    }
    // 서버 응답 읽기 (4바이트 버전); 0x00000000은 버전 협상 실패
    let mut response = [0u8; 4];
    stream.read_exact(&mut response).is_ok() && response != [0, 0, 0, 0]
}
