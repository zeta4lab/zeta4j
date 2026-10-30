//! Bolt 클라이언트 구현.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::{ClientConfig, ClientConnection};

use zeta4j_bolt_protocol::message::{
    AuthToken, BeginMessage, BoltRequest, BoltResponse, HelloMessage, PullMessage, RunMessage,
};
use zeta4j_bolt_protocol::packstream::{PackStreamDecoder, PackStreamEncoder, PackStreamValue};
use zeta4j_bolt_protocol::{BOLT_MAGIC, BoltVersion};
use zeta4j_transport_security::client_tls_config_from_environment;

use super::config::ReconnectConfig;
use super::error::{BoltClientError, BoltClientResult};
use super::result::QueryResult;
use super::stream::{ConnectionStream, TlsStream};

/// Bolt 클라이언트 연결
pub struct BoltClient {
    /// 연결 스트림 (TCP 또는 TLS)
    stream: Option<ConnectionStream>,
    /// 협상된 Bolt 버전
    version: Option<BoltVersion>,
    /// 서버 이름
    server: Option<String>,
    /// 연결 ID
    connection_id: Option<String>,
    /// 현재 데이터베이스
    database: String,
    /// 연결 타임아웃
    timeout: Duration,
    /// 사용자 에이전트
    user_agent: String,
    /// TLS 사용 여부
    use_tls: bool,
    /// Preflight-validated TLS generation and explicit verification name.
    tls_config: Option<Arc<ClientConfig>>,
    tls_server_name: Option<String>,
    transport_error: Option<String>,
    /// 재연결 설정
    reconnect_config: Option<ReconnectConfig>,
    /// 마지막 연결 주소 (재연결용)
    last_address: Option<String>,
    /// Reconnection cannot silently replay a raw password.
    requires_reauthentication: bool,
}

impl BoltClient {
    /// 새 Bolt 클라이언트 생성
    pub fn new() -> Self {
        Self {
            stream: None,
            version: None,
            server: None,
            connection_id: None,
            database: "zeta4j".to_string(),
            timeout: Duration::from_secs(30),
            user_agent: format!("Zeta4j/{}", env!("CARGO_PKG_VERSION")),
            use_tls: false,
            tls_config: None,
            tls_server_name: None,
            transport_error: None,
            reconnect_config: None,
            last_address: None,
            requires_reauthentication: false,
        }
    }

    /// 데이터베이스 설정
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// 타임아웃 설정
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attach a preflight-validated TLS generation and explicit server name.
    pub fn with_transport_config(
        mut self,
        config: Arc<ClientConfig>,
        server_name: impl Into<String>,
    ) -> Self {
        self.tls_config = Some(config);
        self.tls_server_name = Some(server_name.into());
        self.transport_error = None;
        self
    }

    /// 재연결 설정
    ///
    /// 연결이 끊어졌을 때 자동으로 재연결을 시도합니다.
    pub fn with_reconnect(mut self, config: ReconnectConfig) -> Self {
        self.reconnect_config = Some(config);
        self
    }

    /// 재연결 비활성화
    pub fn without_reconnect(mut self) -> Self {
        self.reconnect_config = None;
        self
    }

    /// 재연결 설정 여부
    pub fn has_reconnect(&self) -> bool {
        self.reconnect_config.is_some()
    }

    /// 연결되어 있는지 확인
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// TLS 사용 여부
    pub fn is_tls(&self) -> bool {
        self.use_tls
    }

    /// 서버에 연결
    pub fn connect(&mut self, address: &str) -> BoltClientResult<()> {
        use std::net::ToSocketAddrs;

        if let Some(error) = &self.transport_error {
            return Err(BoltClientError::Tls(error.clone()));
        }

        // 주소 저장 (재연결용)
        self.last_address = Some(address.to_string());

        // 주소 파싱 (bolt://host:port 또는 bolt+s://host:port)
        let (use_tls, host, addr) = self.parse_address(address)?;
        if use_tls && self.tls_config.is_none() {
            let (config, server_name) =
                bolt_transport_from_environment(host.trim_matches(['[', ']']))
                    .map_err(BoltClientError::Tls)?;
            self.tls_config = Some(config);
            self.tls_server_name = Some(server_name);
        }

        // DNS 조회 (hostname:port -> SocketAddr) - IPv4/IPv6 모두 시도
        let addrs: Vec<std::net::SocketAddr> = addr
            .to_socket_addrs()
            .map_err(|e| {
                BoltClientError::ConnectionFailed(format!("Failed to resolve '{}': {}", addr, e))
            })?
            .collect();

        if addrs.is_empty() {
            return Err(BoltClientError::ConnectionFailed(format!(
                "No addresses found for '{}'",
                addr
            )));
        }

        // TCP 연결 - 모든 resolved 주소를 순회하며 연결 시도
        let mut last_err = None;
        let mut tcp_stream = None;
        for socket_addr in &addrs {
            match TcpStream::connect_timeout(socket_addr, self.timeout) {
                Ok(stream) => {
                    tcp_stream = Some(stream);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        let tcp_stream = tcp_stream.ok_or_else(|| {
            BoltClientError::ConnectionFailed(format!(
                "Failed to connect to '{}': {}",
                addr,
                last_err.map(|e| e.to_string()).unwrap_or_default(),
            ))
        })?;

        tcp_stream.set_read_timeout(Some(self.timeout))?;
        tcp_stream.set_write_timeout(Some(self.timeout))?;

        // TLS 연결 또는 Plain 연결
        if use_tls {
            let tls_stream = self.establish_tls(tcp_stream, &host)?;
            self.stream = Some(ConnectionStream::Tls(Box::new(tls_stream)));
            self.use_tls = true;
        } else {
            self.stream = Some(ConnectionStream::Plain(tcp_stream));
            self.use_tls = false;
        }

        // 핸드셰이크 수행
        self.perform_handshake()?;

        Ok(())
    }

    /// Bolt 핸드셰이크 수행
    fn perform_handshake(&mut self) -> BoltClientResult<()> {
        let stream = self.stream.as_mut().ok_or(BoltClientError::NotConnected)?;

        // 1. 매직 넘버 + 버전 제안 전송
        let mut handshake = Vec::with_capacity(20);
        handshake.extend_from_slice(&BOLT_MAGIC);

        // Bolt 5.4, 5.0, 4.4, 4.0 제안 (높은 버전부터)
        handshake.extend_from_slice(&[0x00, 0x04, 0x05, 0x05]); // 5.4
        handshake.extend_from_slice(&[0x00, 0x00, 0x00, 0x05]); // 5.0
        handshake.extend_from_slice(&[0x00, 0x04, 0x04, 0x04]); // 4.4
        handshake.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // 4.0

        stream.write_all(&handshake)?;
        stream.flush()?;

        // 2. 서버 응답 수신 (4바이트 = 협상된 버전)
        let mut response = [0u8; 4];
        stream.read_exact(&mut response)?;

        // 버전 0은 협상 실패
        if response == [0, 0, 0, 0] {
            return Err(BoltClientError::HandshakeFailed(
                "Server does not support any proposed Bolt version".to_string(),
            ));
        }

        // 버전 파싱 (big-endian 4바이트)
        self.version = BoltVersion::from_bytes(response);

        Ok(())
    }

    /// 인증 (HELLO 메시지)
    pub fn authenticate(&mut self, username: &str, password: &str) -> BoltClientResult<()> {
        self.requires_reauthentication = true;
        let auth = AuthToken::basic(username, password);
        self.authenticate_token(auth)
    }

    pub fn authenticate_bearer(&mut self, access_token: &str) -> BoltClientResult<()> {
        self.requires_reauthentication = true;
        self.authenticate_token(AuthToken::bearer(access_token))
    }

    fn authenticate_token(&mut self, auth: AuthToken) -> BoltClientResult<()> {
        let hello = HelloMessage::new(&self.user_agent).with_auth(auth);

        self.send_request(BoltRequest::Hello(hello))?;

        let response = self.receive_response()?;

        match response {
            BoltResponse::Success(success) => {
                self.server = success.server().map(|s| s.to_string());
                self.connection_id = success.connection_id().map(|s| s.to_string());
                Ok(())
            }
            BoltResponse::Failure(failure) => {
                Err(BoltClientError::AuthenticationFailed(failure.message))
            }
            _ => Err(BoltClientError::ProtocolError(
                "Unexpected response to HELLO".to_string(),
            )),
        }
    }

    /// 쿼리 실행
    pub fn execute(
        &mut self,
        query: &str,
        parameters: HashMap<String, PackStreamValue>,
    ) -> BoltClientResult<QueryResult> {
        // RUN 메시지 전송
        let run = RunMessage::new(query)
            .with_parameters(parameters)
            .with_database(&self.database);

        self.send_request(BoltRequest::Run(run))?;

        // RUN 응답 (fields 정보)
        let run_response = self.receive_response()?;
        let columns = match run_response {
            BoltResponse::Success(success) => success.fields().unwrap_or_default(),
            BoltResponse::Failure(failure) => {
                return Err(BoltClientError::QueryFailed {
                    code: failure.code,
                    message: failure.message,
                });
            }
            _ => {
                return Err(BoltClientError::ProtocolError(
                    "Unexpected response to RUN".to_string(),
                ));
            }
        };

        // PULL 메시지 전송
        self.send_request(BoltRequest::Pull(PullMessage::all()))?;

        // RECORD들 수집 후 SUCCESS
        let mut result = QueryResult::new(columns);

        loop {
            let response = self.receive_response()?;
            match response {
                BoltResponse::Record(record) => {
                    result.records.push(record.fields);
                }
                BoltResponse::Success(success) => {
                    result.bookmark = success.bookmark().map(|s| s.to_string());
                    result.stats = success.stats().cloned();
                    break;
                }
                BoltResponse::Failure(failure) => {
                    return Err(BoltClientError::QueryFailed {
                        code: failure.code,
                        message: failure.message,
                    });
                }
                BoltResponse::Ignored => {
                    return Err(BoltClientError::ProtocolError(
                        "Message was ignored".to_string(),
                    ));
                }
            }
        }

        Ok(result)
    }

    /// 트랜잭션 시작
    pub fn begin(&mut self) -> BoltClientResult<()> {
        let begin = BeginMessage::new().with_database(&self.database);
        self.send_request(BoltRequest::Begin(begin))?;

        let response = self.receive_response()?;
        match response {
            BoltResponse::Success(_) => Ok(()),
            BoltResponse::Failure(failure) => Err(BoltClientError::QueryFailed {
                code: failure.code,
                message: failure.message,
            }),
            _ => Err(BoltClientError::ProtocolError(
                "Unexpected response to BEGIN".to_string(),
            )),
        }
    }

    /// 트랜잭션 커밋
    pub fn commit(&mut self) -> BoltClientResult<Option<String>> {
        self.send_request(BoltRequest::Commit)?;

        let response = self.receive_response()?;
        match response {
            BoltResponse::Success(success) => Ok(success.bookmark().map(|s| s.to_string())),
            BoltResponse::Failure(failure) => Err(BoltClientError::QueryFailed {
                code: failure.code,
                message: failure.message,
            }),
            _ => Err(BoltClientError::ProtocolError(
                "Unexpected response to COMMIT".to_string(),
            )),
        }
    }

    /// 트랜잭션 롤백
    pub fn rollback(&mut self) -> BoltClientResult<()> {
        self.send_request(BoltRequest::Rollback)?;

        let response = self.receive_response()?;
        match response {
            BoltResponse::Success(_) => Ok(()),
            BoltResponse::Failure(failure) => Err(BoltClientError::QueryFailed {
                code: failure.code,
                message: failure.message,
            }),
            _ => Err(BoltClientError::ProtocolError(
                "Unexpected response to ROLLBACK".to_string(),
            )),
        }
    }

    /// 연결 초기화 (RESET)
    pub fn reset(&mut self) -> BoltClientResult<()> {
        self.send_request(BoltRequest::Reset)?;

        let response = self.receive_response()?;
        match response {
            BoltResponse::Success(_) => Ok(()),
            _ => Err(BoltClientError::ProtocolError(
                "Unexpected response to RESET".to_string(),
            )),
        }
    }

    /// 연결 종료
    pub fn close(&mut self) -> BoltClientResult<()> {
        if self.stream.is_some() {
            let _ = self.send_request(BoltRequest::Goodbye);
        }
        self.stream = None;
        self.version = None;
        self.server = None;
        self.connection_id = None;
        Ok(())
    }

    /// 연결이 유효한지 확인하고 필요시 재연결
    ///
    /// 재연결 설정이 활성화되어 있고 연결이 끊어진 경우 자동으로 재연결을 시도합니다.
    pub fn ensure_connected(&mut self) -> BoltClientResult<()> {
        // 이미 연결되어 있으면 OK
        if self.is_connected() {
            return Ok(());
        }

        // 재연결 설정이 없으면 에러
        if self.reconnect_config.is_none() {
            return Err(BoltClientError::NotConnected);
        }

        // 재연결에 필요한 정보가 없으면 에러
        if self.last_address.is_none() {
            return Err(BoltClientError::ConnectionFailed(
                "Cannot reconnect: no previous connection address".to_string(),
            ));
        }

        self.try_reconnect()
    }

    /// 재연결 시도
    ///
    /// 설정된 재시도 횟수만큼 지수 백오프로 재연결을 시도합니다.
    pub fn try_reconnect(&mut self) -> BoltClientResult<()> {
        if self.requires_reauthentication {
            return Err(BoltClientError::AuthenticationFailed(
                "connection closed; explicit reauthentication is required".to_string(),
            ));
        }
        let config = self.reconnect_config.clone().ok_or_else(|| {
            BoltClientError::ConnectionFailed("Reconnect not configured".to_string())
        })?;

        let address = self.last_address.clone().ok_or_else(|| {
            BoltClientError::ConnectionFailed("No previous connection address".to_string())
        })?;

        let mut last_error = None;

        for attempt in 0..=config.max_retries {
            if attempt > 0 {
                // 지수 백오프 대기
                let delay = config.delay_for_attempt(attempt - 1);
                std::thread::sleep(delay);
            }

            // 연결 시도
            match self.connect(&address) {
                Ok(()) => {
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    // 다음 재시도를 위해 연결 상태 초기화
                    self.stream = None;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            BoltClientError::ConnectionFailed(format!(
                "Failed to reconnect after {} attempts",
                config.max_retries + 1
            ))
        }))
    }

    /// 연결 상태 확인 (ping)
    ///
    /// RESET 메시지를 보내 연결이 살아있는지 확인합니다.
    pub fn ping(&mut self) -> bool {
        if !self.is_connected() {
            return false;
        }
        self.reset().is_ok()
    }

    /// 서버 이름 반환
    pub fn server(&self) -> Option<&str> {
        self.server.as_deref()
    }

    /// Bolt 버전 반환
    pub fn version(&self) -> Option<&BoltVersion> {
        self.version.as_ref()
    }

    /// 현재 데이터베이스 반환
    pub fn database(&self) -> &str {
        &self.database
    }

    // ==================== Private Methods ====================

    /// 주소 파싱 - (use_tls, hostname, host:port) 반환
    fn parse_address(&self, address: &str) -> BoltClientResult<(bool, String, String)> {
        let (use_tls, rest) = if let Some(rest) = address.strip_prefix("bolt+s://") {
            (true, rest)
        } else if let Some(rest) = address.strip_prefix("bolt://") {
            (false, rest)
        } else {
            (false, address)
        };

        // user:pass@ 부분 제거
        let host_part = if let Some(at_pos) = rest.find('@') {
            &rest[at_pos + 1..]
        } else {
            rest
        };

        // /database 부분 제거
        let host_port = if let Some(slash_pos) = host_part.find('/') {
            &host_part[..slash_pos]
        } else {
            host_part
        };

        // 호스트명 추출 (포트 제거)
        let hostname = if let Some(colon_pos) = host_port.rfind(':') {
            host_port[..colon_pos].to_string()
        } else {
            host_port.to_string()
        };

        // 포트가 없으면 기본 포트 추가
        let addr = if host_port.contains(':') {
            host_port.to_string()
        } else {
            format!("{}:7687", host_port)
        };

        Ok((use_tls, hostname, addr))
    }

    /// TLS connection using only the preflight-validated generation.
    fn establish_tls(&self, tcp_stream: TcpStream, _hostname: &str) -> BoltClientResult<TlsStream> {
        use rustls::pki_types::ServerName;

        let config = self.tls_config.clone().ok_or_else(|| {
            BoltClientError::Tls("SV-TLS-001: Bolt TLS generation is missing".to_string())
        })?;
        let expected_name = self.tls_server_name.as_ref().ok_or_else(|| {
            BoltClientError::Tls("SV-TLS-004: Bolt server name is missing".to_string())
        })?;
        let server_name = ServerName::try_from(expected_name.clone()).map_err(|_| {
            BoltClientError::Tls("SV-TLS-004: invalid Bolt server name".to_string())
        })?;

        // TLS 연결 생성
        let conn = ClientConnection::new(config, server_name)
            .map_err(|e| BoltClientError::Tls(format!("TLS connection failed: {}", e)))?;

        // StreamOwned으로 래핑
        let tls_stream = rustls::StreamOwned::new(conn, tcp_stream);

        Ok(tls_stream)
    }

    /// 요청 메시지 전송
    fn send_request(&mut self, request: BoltRequest) -> BoltClientResult<()> {
        let stream = self.stream.as_mut().ok_or(BoltClientError::NotConnected)?;

        // PackStream 인코딩
        let structure = request.to_structure();
        let mut encoder = PackStreamEncoder::new();
        encoder.encode_structure(&structure)?;
        let data = encoder.into_bytes();

        // Chunked 포맷으로 전송 (2바이트 길이 + 데이터 + 0x0000 종료)
        let len = data.len() as u16;
        let mut message = Vec::with_capacity(data.len() + 4);
        message.extend_from_slice(&len.to_be_bytes());
        message.extend_from_slice(&data);
        message.extend_from_slice(&[0x00, 0x00]); // 메시지 종료 마커

        stream.write_all(&message)?;
        stream.flush()?;

        Ok(())
    }

    /// 응답 메시지 수신
    fn receive_response(&mut self) -> BoltClientResult<BoltResponse> {
        let stream = self.stream.as_mut().ok_or(BoltClientError::NotConnected)?;

        // Chunked 데이터 수신
        let mut full_data = Vec::new();

        loop {
            // 청크 길이 읽기 (2바이트)
            let mut len_bytes = [0u8; 2];
            stream.read_exact(&mut len_bytes)?;
            let chunk_len = u16::from_be_bytes(len_bytes) as usize;

            if chunk_len == 0 {
                // 메시지 종료
                break;
            }

            // 청크 데이터 읽기
            let mut chunk = vec![0u8; chunk_len];
            stream.read_exact(&mut chunk)?;
            full_data.extend_from_slice(&chunk);
        }

        if full_data.is_empty() {
            return Err(BoltClientError::ProtocolError(
                "Empty response from server".to_string(),
            ));
        }

        // PackStream 디코딩
        let mut decoder = PackStreamDecoder::new(&full_data);
        let value = decoder.decode().map_err(|e| {
            BoltClientError::ProtocolError(format!("Failed to decode response: {:?}", e))
        })?;

        // Structure 추출
        let structure = value.as_structure().ok_or_else(|| {
            BoltClientError::ProtocolError("Expected structure in response".to_string())
        })?;

        // BoltResponse로 변환
        let response = BoltResponse::from_structure(structure).map_err(|e| {
            BoltClientError::ProtocolError(format!("Invalid response structure: {:?}", e))
        })?;

        Ok(response)
    }
}

fn bolt_transport_from_environment(host: &str) -> Result<(Arc<ClientConfig>, String), String> {
    let (config, server_name) =
        client_tls_config_from_environment(&[], host).map_err(|error| error.to_string())?;
    Ok((Arc::new(config), server_name))
}

impl Default for BoltClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BoltClient {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod transport_tests {
    use super::{BoltClient, BoltClientError};

    #[test]
    fn plaintext_bolt_is_not_limited_to_numeric_loopback() {
        let mut client = BoltClient::new();
        let error = client.connect("bolt://localhost:1").err();
        assert!(
            !matches!(error, Some(BoltClientError::Tls(_))),
            "plaintext Bolt was rejected by transport policy: {error:?}"
        );
    }
}
