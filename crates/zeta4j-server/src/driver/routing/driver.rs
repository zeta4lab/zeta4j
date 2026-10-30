//! 라우팅 드라이버
//!
//! 클러스터 환경에서 자동 라우팅을 지원하는 드라이버입니다.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use super::super::driver::{AuthToken, DriverConfig, ServerAddress, TrustStrategy};
use super::super::error::{DriverError, DriverResult};
use super::super::pool::{ConnectionPool, PoolConfig};
use super::super::session::{AccessMode, Session, SessionConfig};
use super::policy::{RoutingPolicy, ServerSelector};
use super::table::RoutingTable;

/// 라우팅 드라이버
///
/// zeta4j:// 스킴을 사용하여 클러스터에 연결합니다.
pub struct RoutingDriver {
    /// 초기 라우터 주소 목록
    initial_routers: Vec<ServerAddress>,
    /// 데이터베이스별 라우팅 테이블
    routing_tables: RwLock<HashMap<String, RoutingTable>>,
    /// 서버별 연결 풀
    pools: RwLock<HashMap<ServerAddress, Arc<ConnectionPool>>>,
    /// 드라이버 설정
    config: DriverConfig,
    /// 서버 선택기
    selector: ServerSelector,
    /// 열린 상태
    open: Arc<RwLock<bool>>,
    /// 라우팅 정책
    routing_policy: RoutingPolicy,
}

impl RoutingDriver {
    /// 새 라우팅 드라이버 생성
    ///
    /// URI 형식: `zeta4j://host1:port1,host2:port2,...`
    pub fn new(uri: &str, auth: AuthToken) -> DriverResult<Self> {
        let config = DriverConfig::new(uri, auth)?;
        Self::with_config(config)
    }

    /// 설정으로 라우팅 드라이버 생성
    pub fn with_config(config: DriverConfig) -> DriverResult<Self> {
        // 초기 라우터 파싱
        let initial_routers = vec![config.address.clone()];

        Ok(Self {
            initial_routers,
            routing_tables: RwLock::new(HashMap::new()),
            pools: RwLock::new(HashMap::new()),
            config,
            selector: ServerSelector::default(),
            open: Arc::new(RwLock::new(true)),
            routing_policy: RoutingPolicy::RoundRobin,
        })
    }

    /// 다중 라우터로 드라이버 생성
    pub fn with_routers(
        routers: Vec<ServerAddress>,
        auth: AuthToken,
        encrypted: bool,
    ) -> DriverResult<Self> {
        if routers.is_empty() {
            return Err(DriverError::configuration(
                "At least one router is required",
            ));
        }

        let config = DriverConfig {
            address: routers[0].clone(),
            auth,
            encrypted,
            trust_strategy: TrustStrategy::default(),
            max_connection_pool_size: 100,
            connection_acquisition_timeout: Duration::from_secs(60),
            connection_timeout: Duration::from_secs(30),
            max_transaction_retry_time: Duration::from_secs(30),
            keep_alive: true,
            user_agent: format!("Zeta4j/{}", env!("CARGO_PKG_VERSION")),
            fetch_size: 1000,
        };

        Ok(Self {
            initial_routers: routers,
            routing_tables: RwLock::new(HashMap::new()),
            pools: RwLock::new(HashMap::new()),
            config,
            selector: ServerSelector::default(),
            open: Arc::new(RwLock::new(true)),
            routing_policy: RoutingPolicy::RoundRobin,
        })
    }

    /// 라우팅 정책 설정
    pub fn with_routing_policy(mut self, policy: RoutingPolicy) -> Self {
        self.routing_policy = policy;
        self.selector = ServerSelector::new(policy);
        self
    }

    /// 세션 생성
    pub async fn session(&self, config: SessionConfig) -> DriverResult<Session> {
        self.ensure_open()?;

        let database = config
            .database
            .clone()
            .unwrap_or_else(|| "zeta4j".to_string());

        // 라우팅 테이블 확인/갱신
        self.ensure_routing_table(&database).await?;

        // 접근 모드에 따른 서버 선택
        let address = self.select_server(&database, config.default_access_mode)?;

        // 해당 서버의 풀 가져오기
        let pool = self.get_or_create_pool(&address)?;

        Session::new(Arc::new(self.config.clone()), pool, config)
    }

    /// 기본 설정으로 세션 생성
    pub async fn default_session(&self) -> DriverResult<Session> {
        self.session(SessionConfig::default()).await
    }

    /// 드라이버 설정
    pub fn config(&self) -> &DriverConfig {
        &self.config
    }

    /// 드라이버 종료
    pub async fn close(&self) -> DriverResult<()> {
        {
            let mut open = self.open.write();
            if !*open {
                return Ok(());
            }
            *open = false;
        } // Guard dropped before await

        // 모든 풀 닫기
        let pools_to_close: Vec<_> = {
            let pools = self.pools.read();
            pools.values().cloned().collect()
        }; // Guard dropped before await

        for pool in pools_to_close {
            pool.close().await?;
        }

        Ok(())
    }

    /// 연결 테스트
    pub async fn verify_connectivity(&self) -> DriverResult<()> {
        self.ensure_open()?;

        // 초기 라우터 중 하나에 연결
        for router in &self.initial_routers {
            let pool = self.get_or_create_pool(router)?;
            if pool.verify_connectivity().await.is_ok() {
                return Ok(());
            }
        }

        Err(DriverError::service_unavailable(
            "Unable to connect to any router",
        ))
    }

    /// 라우팅 테이블 조회
    pub fn get_routing_table(&self, database: &str) -> Option<RoutingTable> {
        self.routing_tables.read().get(database).cloned()
    }

    /// 열린 상태 확인
    fn ensure_open(&self) -> DriverResult<()> {
        if *self.open.read() {
            Ok(())
        } else {
            Err(DriverError::session("Driver is closed"))
        }
    }

    /// 라우팅 테이블 확인 및 갱신
    async fn ensure_routing_table(&self, database: &str) -> DriverResult<()> {
        let needs_refresh = {
            let tables = self.routing_tables.read();
            match tables.get(database) {
                Some(table) => table.is_expired() || table.needs_refresh(),
                None => true,
            }
        };

        if needs_refresh {
            self.refresh_routing_table(database).await?;
        }

        Ok(())
    }

    /// 라우팅 테이블 갱신
    async fn refresh_routing_table(&self, database: &str) -> DriverResult<()> {
        // ROUTE 메시지를 통해 서버에서 라우팅 테이블 조회 시도
        // 실패 시 초기 라우터를 기반으로 테이블 생성 (fallback)

        let routing_info = self.fetch_routing_info(database).await;

        let mut tables = self.routing_tables.write();

        let table = match routing_info {
            Ok(info) => {
                // 서버에서 받은 라우팅 정보로 테이블 생성
                let mut table = RoutingTable::with_initial_routers(database, info.routers.clone());
                table.set_ttl(info.ttl);

                for writer in info.writers {
                    table.add_writer(writer);
                }
                for reader in info.readers {
                    table.add_reader(reader);
                }

                tables.insert(database.to_string(), table.clone());
                table
            }
            Err(_) => {
                // Fallback: 초기 라우터 기반 테이블 생성
                if let Some(existing) = tables.get_mut(database) {
                    // 기존 테이블 갱신
                    existing.mark_updated();
                    existing.set_ttl(300); // 5분
                    existing.clone()
                } else {
                    // 새 테이블 생성 (단일 서버 모드)
                    let mut table =
                        RoutingTable::with_initial_routers(database, self.initial_routers.clone());
                    table.set_ttl(300);

                    // 초기 라우터를 모든 역할에 추가
                    for router in &self.initial_routers {
                        table.add_writer(router.clone());
                        table.add_reader(router.clone());
                    }

                    tables.insert(database.to_string(), table.clone());
                    table
                }
            }
        };

        drop(tables);

        // 라우팅 테이블의 모든 서버에 대해 풀 생성
        for addr in table
            .routers
            .iter()
            .chain(table.writers.iter())
            .chain(table.readers.iter())
        {
            let _ = self.get_or_create_pool(addr);
        }

        Ok(())
    }

    /// 서버에서 라우팅 정보 조회 (ROUTE 메시지)
    async fn fetch_routing_info(
        &self,
        database: &str,
    ) -> DriverResult<super::super::bolt::RoutingInfo> {
        Self::fetch_routing_info_async(&self.initial_routers, database, &self.config).await
    }

    /// 비동기 라우팅 정보 조회
    async fn fetch_routing_info_async(
        routers: &[ServerAddress],
        database: &str,
        config: &DriverConfig,
    ) -> DriverResult<super::super::bolt::RoutingInfo> {
        use super::super::bolt::{BoltClient, to_bolt_auth};

        for router in routers {
            let addr = router.to_socket_addr();

            // 연결 시도
            let client_result =
                tokio::time::timeout(config.connection_timeout, BoltClient::connect(&addr)).await;

            let mut client = match client_result {
                Ok(Ok(c)) => c,
                _ => continue, // 다음 라우터 시도
            };

            // 인증
            let bolt_auth = to_bolt_auth(&config.auth);
            if client.hello(Some(bolt_auth)).await.is_err() {
                continue;
            }

            // ROUTE 메시지 전송
            match client.route(Some(database), vec![]).await {
                Ok(info) => {
                    let _ = client.close().await;
                    return Ok(info);
                }
                Err(_) => {
                    let _ = client.close().await;
                    continue;
                }
            }
        }

        Err(DriverError::service_unavailable(
            "Failed to get routing table from any router",
        ))
    }

    /// 접근 모드에 따른 서버 선택
    fn select_server(&self, database: &str, mode: AccessMode) -> DriverResult<ServerAddress> {
        let tables = self.routing_tables.read();
        let table = tables
            .get(database)
            .ok_or_else(|| DriverError::service_unavailable("No routing table available"))?;

        let servers = match mode {
            AccessMode::Write => &table.writers,
            AccessMode::Read => &table.readers,
        };

        self.selector.select(servers).cloned().ok_or_else(|| {
            DriverError::service_unavailable(format!("No {:?} servers available", mode))
        })
    }

    /// 서버에 대한 연결 풀 가져오기 또는 생성
    fn get_or_create_pool(&self, address: &ServerAddress) -> DriverResult<Arc<ConnectionPool>> {
        // 읽기 락으로 먼저 확인
        {
            let pools = self.pools.read();
            if let Some(pool) = pools.get(address) {
                return Ok(pool.clone());
            }
        }

        // 쓰기 락으로 생성
        let mut pools = self.pools.write();

        // 다시 확인 (경쟁 조건 방지)
        if let Some(pool) = pools.get(address) {
            return Ok(pool.clone());
        }

        // 새 풀 생성
        let pool_config = PoolConfig {
            max_size: self.config.max_connection_pool_size / self.initial_routers.len().max(1),
            min_idle: 0,
            warmup_on_init: false,
            warmup_size: 0,
            max_lifetime: Duration::from_secs(3600),
            idle_timeout: Duration::from_secs(300),
            connection_timeout: self.config.connection_timeout,
            validation_timeout: Duration::from_secs(5),
        };

        let pool = Arc::new(ConnectionPool::new(address.clone(), pool_config));
        pools.insert(address.clone(), pool.clone());

        Ok(pool)
    }

    /// 서버를 실패로 표시
    pub fn mark_server_failed(&self, database: &str, address: &ServerAddress) {
        let mut tables = self.routing_tables.write();
        if let Some(table) = tables.get_mut(database) {
            table.remove_server(address);
        }
    }

    /// 초기 라우터 목록
    pub fn initial_routers(&self) -> &[ServerAddress] {
        &self.initial_routers
    }

    /// 드라이버 메트릭
    pub fn metrics(&self) -> RoutingDriverMetrics {
        let pools = self.pools.read();
        let mut total_size = 0;
        let mut total_idle = 0;
        let mut total_in_use = 0;

        for pool in pools.values() {
            let m = pool.metrics();
            total_size += m.size;
            total_idle += m.idle;
            total_in_use += m.in_use;
        }

        RoutingDriverMetrics {
            pool_count: pools.len(),
            total_pool_size: total_size,
            total_idle_connections: total_idle,
            total_in_use_connections: total_in_use,
            routing_table_count: self.routing_tables.read().len(),
        }
    }
}

impl fmt::Debug for RoutingDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingDriver")
            .field("initial_routers", &self.initial_routers)
            .field("open", &*self.open.read())
            .field("routing_policy", &self.routing_policy)
            .finish()
    }
}

/// 라우팅 드라이버 메트릭
#[derive(Debug, Clone, Default)]
pub struct RoutingDriverMetrics {
    /// 연결 풀 수
    pub pool_count: usize,
    /// 전체 풀 크기
    pub total_pool_size: usize,
    /// 전체 유휴 연결 수
    pub total_idle_connections: usize,
    /// 전체 사용 중인 연결 수
    pub total_in_use_connections: usize,
    /// 라우팅 테이블 수
    pub routing_table_count: usize,
}

/// URI가 라우팅 드라이버용인지 확인
pub fn is_routing_uri(uri: &str) -> bool {
    uri.starts_with("zeta4j://")
        || uri.starts_with("zeta4j+s://")
        || uri.starts_with("zeta4j+ssc://")
}

/// URI에서 다중 라우터 파싱
pub fn parse_routing_uri(uri: &str) -> DriverResult<Vec<ServerAddress>> {
    let uri = uri
        .trim_start_matches("zeta4j://")
        .trim_start_matches("zeta4j+s://")
        .trim_start_matches("zeta4j+ssc://");

    let mut routers = Vec::new();

    for addr_str in uri.split(',') {
        let addr_str = addr_str.trim();
        if addr_str.is_empty() {
            continue;
        }

        let parts: Vec<&str> = addr_str.split(':').collect();
        let (host, port) = match parts.len() {
            1 => (parts[0], 9045),
            2 => {
                let port = parts[1]
                    .parse()
                    .map_err(|_| DriverError::configuration("Invalid port"))?;
                (parts[0], port)
            }
            _ => return Err(DriverError::configuration("Invalid server address")),
        };

        routers.push(ServerAddress::new(host, port));
    }

    if routers.is_empty() {
        return Err(DriverError::configuration("No routers specified"));
    }

    Ok(routers)
}
