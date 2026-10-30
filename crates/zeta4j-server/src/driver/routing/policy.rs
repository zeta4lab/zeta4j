//! 라우팅 정책
//!
//! 서버 선택 전략을 정의합니다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::RwLock;

use super::super::driver::ServerAddress;

/// 라우팅 정책
#[derive(Debug, Clone, Copy, Default)]
pub enum RoutingPolicy {
    /// 라운드 로빈 (기본값)
    #[default]
    RoundRobin,
    /// 최소 연결
    LeastConnections,
    /// 랜덤
    Random,
}

/// 서버별 연결 카운터
#[derive(Debug, Default)]
pub struct ConnectionCounter {
    /// 서버별 활성 연결 수
    counts: RwLock<HashMap<ServerAddress, usize>>,
}

impl ConnectionCounter {
    /// 새 카운터 생성
    pub fn new() -> Self {
        Self {
            counts: RwLock::new(HashMap::new()),
        }
    }

    /// 연결 획득 시 카운트 증가
    pub fn acquire(&self, server: &ServerAddress) {
        let mut counts = self.counts.write();
        *counts.entry(server.clone()).or_insert(0) += 1;
    }

    /// 연결 해제 시 카운트 감소
    pub fn release(&self, server: &ServerAddress) {
        let mut counts = self.counts.write();
        if let Some(count) = counts.get_mut(server) {
            *count = count.saturating_sub(1);
        }
    }

    /// 특정 서버의 연결 수 조회
    pub fn get(&self, server: &ServerAddress) -> usize {
        self.counts.read().get(server).copied().unwrap_or(0)
    }

    /// 최소 연결 서버 선택
    pub fn select_least<'a>(&self, servers: &'a [ServerAddress]) -> Option<&'a ServerAddress> {
        if servers.is_empty() {
            return None;
        }

        let counts = self.counts.read();
        servers
            .iter()
            .min_by_key(|s| counts.get(*s).copied().unwrap_or(0))
    }

    /// 모든 카운트 리셋
    pub fn reset(&self) {
        self.counts.write().clear();
    }

    /// 현재 연결 수 스냅샷
    pub fn snapshot(&self) -> HashMap<ServerAddress, usize> {
        self.counts.read().clone()
    }
}

/// 서버 선택기
pub struct ServerSelector {
    /// 라우팅 정책
    policy: RoutingPolicy,
    /// 라운드 로빈 인덱스
    round_robin_index: AtomicUsize,
    /// 연결 카운터 (LeastConnections용)
    connection_counter: ConnectionCounter,
}

impl ServerSelector {
    /// 새 선택기 생성
    pub fn new(policy: RoutingPolicy) -> Self {
        Self {
            policy,
            round_robin_index: AtomicUsize::new(0),
            connection_counter: ConnectionCounter::new(),
        }
    }

    /// 기본 선택기 (라운드 로빈)
    pub fn round_robin() -> Self {
        Self::new(RoutingPolicy::RoundRobin)
    }

    /// 최소 연결 선택기
    pub fn least_connections() -> Self {
        Self::new(RoutingPolicy::LeastConnections)
    }

    /// 랜덤 선택기
    pub fn random() -> Self {
        Self::new(RoutingPolicy::Random)
    }

    /// 서버 목록에서 하나 선택
    pub fn select<'a>(&self, servers: &'a [ServerAddress]) -> Option<&'a ServerAddress> {
        if servers.is_empty() {
            return None;
        }

        match self.policy {
            RoutingPolicy::RoundRobin => {
                let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
                Some(&servers[index % servers.len()])
            }
            RoutingPolicy::Random => {
                use rand::Rng;
                let index = rand::rng().random_range(0..servers.len());
                Some(&servers[index])
            }
            RoutingPolicy::LeastConnections => {
                // 최소 연결 서버 선택
                self.connection_counter.select_least(servers)
            }
        }
    }

    /// 특정 서버를 제외하고 선택
    pub fn select_except<'a>(
        &self,
        servers: &'a [ServerAddress],
        except: &ServerAddress,
    ) -> Option<&'a ServerAddress> {
        let filtered: Vec<&ServerAddress> = servers.iter().filter(|s| *s != except).collect();

        if filtered.is_empty() {
            return None;
        }

        match self.policy {
            RoutingPolicy::RoundRobin => {
                let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
                Some(filtered[index % filtered.len()])
            }
            RoutingPolicy::Random => {
                use rand::Rng;
                let index = rand::rng().random_range(0..filtered.len());
                Some(filtered[index])
            }
            RoutingPolicy::LeastConnections => {
                // filtered는 참조 슬라이스이므로 별도 처리
                let counts = self.connection_counter.snapshot();
                filtered
                    .into_iter()
                    .min_by_key(|s| counts.get(*s).copied().unwrap_or(0))
            }
        }
    }

    /// 연결 획득 알림 (LeastConnections용)
    pub fn on_connection_acquired(&self, server: &ServerAddress) {
        self.connection_counter.acquire(server);
    }

    /// 연결 해제 알림 (LeastConnections용)
    pub fn on_connection_released(&self, server: &ServerAddress) {
        self.connection_counter.release(server);
    }

    /// 특정 서버의 연결 수 조회
    pub fn connection_count(&self, server: &ServerAddress) -> usize {
        self.connection_counter.get(server)
    }

    /// 인덱스 리셋
    pub fn reset(&self) {
        self.round_robin_index.store(0, Ordering::Relaxed);
        self.connection_counter.reset();
    }

    /// 현재 정책 조회
    pub fn policy(&self) -> RoutingPolicy {
        self.policy
    }
}

impl Default for ServerSelector {
    fn default() -> Self {
        Self::round_robin()
    }
}
