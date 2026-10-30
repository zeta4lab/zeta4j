//! # 트랜잭션 진단 도구
//!
//! 트랜잭션 시스템의 상태를 분석하고 문제를 진단하는 도구입니다.
//!
//! ## 주요 기능
//!
//! - **트랜잭션 덤프**: 활성 트랜잭션의 상세 상태 출력
//! - **락 체인 분석**: 락 대기 체인 및 데드락 시각화
//! - **WAL 분석**: WAL 레코드 통계 및 상태
//! - **MVCC 분석**: 스냅샷 및 버전 정보
//! - **통계 수집**: 트랜잭션/락 통계 및 메트릭
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::diagnostics::{DiagnosticsDumper, LockChainAnalyzer};
//!
//! let dumper = DiagnosticsDumper::new(&orchestrator);
//!
//! // 전체 시스템 상태 덤프
//! let dump = dumper.full_dump();
//! println!("{}", dump);
//!
//! // 락 체인 분석
//! let analyzer = LockChainAnalyzer::new(&lock_manager);
//! let chains = analyzer.find_wait_chains();
//!
//! // 트랜잭션 통계
//! let tx_stats = dumper.transaction_stats();
//! println!("Active: {}, Avg Duration: {}ms", tx_stats.active_count, tx_stats.avg_duration_ms);
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Instant;

use super::{
    IsolationLevel, LockManager, MvccEngine, TransactionId, TransactionManager, lock::LockResource,
};

/// 트랜잭션 덤프 정보
#[derive(Debug, Clone)]
pub struct TransactionDump {
    /// 덤프 생성 시간
    pub timestamp: Instant,
    /// 활성 트랜잭션 목록
    pub active_transactions: Vec<TransactionInfo>,
    /// 총 트랜잭션 수
    pub total_count: usize,
    /// 상태별 카운트
    pub state_counts: HashMap<String, usize>,
}

/// 개별 트랜잭션 정보
#[derive(Debug, Clone)]
pub struct TransactionInfo {
    /// 트랜잭션 ID
    pub id: TransactionId,
    /// 현재 상태
    pub state: String,
    /// 보유 중인 락 수
    pub held_locks: usize,
    /// 대기 중인 락 정보
    pub waiting_for: Option<LockWaitInfo>,
    /// 격리 수준
    pub isolation_level: IsolationLevel,
    /// 시작 시간
    pub started_at: Instant,
    /// 마지막 활동 시간
    pub last_activity: Instant,
    /// 실행된 문장 수
    pub statements_executed: u64,
    /// 경과 시간 (밀리초)
    pub elapsed_ms: u64,
}

/// 락 대기 정보
#[derive(Debug, Clone)]
pub struct LockWaitInfo {
    /// 대기 중인 리소스
    pub resource: String,
    /// 요청한 락 모드
    pub requested_mode: String,
    /// 현재 락 홀더
    pub holders: Vec<TransactionId>,
    /// 대기 시작 시간
    pub wait_started: Instant,
}

/// 대기자 정보 (Phase 2)
#[derive(Debug, Clone)]
pub struct WaiterInfo {
    /// 대기 중인 트랜잭션 ID
    pub tx_id: TransactionId,
    /// 요청한 락 모드
    pub lock_mode: String,
    /// 요청 시간
    pub requested_at: Instant,
    /// 대기 시간 (밀리초)
    pub wait_time_ms: u64,
}

/// 데드락 정보 (Phase 2)
#[derive(Debug, Clone)]
pub struct DeadlockInfo {
    /// 감지 시간
    pub detected_at: Instant,
    /// 사이클에 포함된 트랜잭션
    pub cycle: Vec<TransactionId>,
    /// 희생 트랜잭션 (롤백 대상)
    pub victim: TransactionId,
    /// 관련 리소스
    pub resources: Vec<String>,
}

/// 트랜잭션 통계 (Phase 4)
#[derive(Debug, Clone, Default)]
pub struct TransactionStats {
    /// 현재 활성 트랜잭션 수
    pub active_count: u64,
    /// 커밋된 트랜잭션 수
    pub committed_count: u64,
    /// 중단된 트랜잭션 수
    pub aborted_count: u64,
    /// 평균 트랜잭션 지속 시간 (밀리초)
    pub avg_duration_ms: f64,
    /// 최대 트랜잭션 지속 시간 (밀리초)
    pub max_duration_ms: u64,
    /// 총 락 대기 시간 (밀리초)
    pub lock_wait_time_ms: u64,
}

/// 락 통계 (Phase 4)
#[derive(Debug, Clone, Default)]
pub struct LockStats {
    /// 총 락 수
    pub total_locks: u64,
    /// Shared 락 수
    pub shared_locks: u64,
    /// Exclusive 락 수
    pub exclusive_locks: u64,
    /// 락 충돌 수
    pub lock_conflicts: u64,
    /// 감지된 데드락 수
    pub deadlocks_detected: u64,
    /// 평균 대기 시간 (밀리초)
    pub avg_wait_time_ms: f64,
}

impl fmt::Display for TransactionDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Transaction System Dump ===")?;
        writeln!(f, "Timestamp: {:?}", self.timestamp)?;
        writeln!(f, "Total Transactions: {}", self.total_count)?;
        writeln!(f)?;

        writeln!(f, "State Distribution:")?;
        for (state, count) in &self.state_counts {
            writeln!(f, "  {}: {}", state, count)?;
        }
        writeln!(f)?;

        if !self.active_transactions.is_empty() {
            writeln!(f, "Active Transactions:")?;
            for tx in &self.active_transactions {
                writeln!(
                    f,
                    "  TX-{}: state={}, locks={}, isolation={:?}, elapsed={}ms, stmts={}",
                    tx.id,
                    tx.state,
                    tx.held_locks,
                    tx.isolation_level,
                    tx.elapsed_ms,
                    tx.statements_executed
                )?;
                if let Some(wait) = &tx.waiting_for {
                    writeln!(
                        f,
                        "    Waiting for: {} ({})",
                        wait.resource, wait.requested_mode
                    )?;
                    writeln!(f, "    Held by: {:?}", wait.holders)?;
                }
            }
        }

        Ok(())
    }
}

impl fmt::Display for TransactionStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Transaction Statistics ===")?;
        writeln!(f, "Active: {}", self.active_count)?;
        writeln!(f, "Committed: {}", self.committed_count)?;
        writeln!(f, "Aborted: {}", self.aborted_count)?;
        writeln!(f, "Avg Duration: {:.2}ms", self.avg_duration_ms)?;
        writeln!(f, "Max Duration: {}ms", self.max_duration_ms)?;
        writeln!(f, "Lock Wait Time: {}ms", self.lock_wait_time_ms)?;
        Ok(())
    }
}

impl fmt::Display for LockStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Lock Statistics ===")?;
        writeln!(f, "Total Locks: {}", self.total_locks)?;
        writeln!(f, "Shared: {}", self.shared_locks)?;
        writeln!(f, "Exclusive: {}", self.exclusive_locks)?;
        writeln!(f, "Conflicts: {}", self.lock_conflicts)?;
        writeln!(f, "Deadlocks: {}", self.deadlocks_detected)?;
        writeln!(f, "Avg Wait Time: {:.2}ms", self.avg_wait_time_ms)?;
        Ok(())
    }
}

/// 락 체인 정보
#[derive(Debug, Clone)]
pub struct LockChain {
    /// 체인의 시작 트랜잭션
    pub start_tx: TransactionId,
    /// 체인 경로 (tx_id -> waiting_for -> tx_id -> ...)
    pub path: Vec<LockChainNode>,
    /// 사이클 여부 (데드락)
    pub is_cycle: bool,
    /// 체인 길이
    pub length: usize,
}

/// 락 체인 노드
#[derive(Debug, Clone)]
pub struct LockChainNode {
    /// 트랜잭션 ID
    pub tx_id: TransactionId,
    /// 대기 중인 리소스
    pub waiting_resource: Option<String>,
    /// 요청한 락 모드
    pub requested_mode: Option<String>,
}

impl fmt::Display for LockChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_cycle {
            write!(f, "[DEADLOCK] ")?;
        }
        write!(f, "Chain (len={}): ", self.length)?;

        for (i, node) in self.path.iter().enumerate() {
            if i > 0 {
                write!(f, " -> ")?;
            }
            write!(f, "TX-{}", node.tx_id)?;
            if let Some(ref res) = node.waiting_resource {
                write!(f, " (wait: {})", res)?;
            }
        }

        if self.is_cycle {
            write!(f, " -> TX-{} [CYCLE]", self.start_tx)?;
        }

        Ok(())
    }
}

/// 락 체인 분석 결과
#[derive(Debug, Clone)]
pub struct LockChainAnalysis {
    /// 발견된 모든 대기 체인
    pub chains: Vec<LockChain>,
    /// 데드락 수
    pub deadlock_count: usize,
    /// 가장 긴 체인 길이
    pub max_chain_length: usize,
    /// 대기 중인 트랜잭션 수
    pub waiting_transactions: usize,
    /// 분석 시간
    pub analysis_time_ms: u64,
}

impl fmt::Display for LockChainAnalysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Lock Chain Analysis ===")?;
        writeln!(f, "Analysis Time: {}ms", self.analysis_time_ms)?;
        writeln!(f, "Waiting Transactions: {}", self.waiting_transactions)?;
        writeln!(f, "Total Chains: {}", self.chains.len())?;
        writeln!(f, "Deadlocks: {}", self.deadlock_count)?;
        writeln!(f, "Max Chain Length: {}", self.max_chain_length)?;
        writeln!(f)?;

        if !self.chains.is_empty() {
            writeln!(f, "Chains:")?;
            for chain in &self.chains {
                writeln!(f, "  {}", chain)?;
            }
        }

        Ok(())
    }
}

/// 트랜잭션 진단 덤퍼
///
/// 트랜잭션 시스템의 상태를 덤프합니다.
pub struct DiagnosticsDumper<'a> {
    tx_manager: &'a TransactionManager,
    lock_manager: &'a LockManager,
    mvcc: &'a MvccEngine,
}

impl<'a> DiagnosticsDumper<'a> {
    /// 새 덤퍼 생성
    pub fn new(
        tx_manager: &'a TransactionManager,
        lock_manager: &'a LockManager,
        mvcc: &'a MvccEngine,
    ) -> Self {
        Self {
            tx_manager,
            lock_manager,
            mvcc,
        }
    }

    /// 전체 시스템 덤프 생성
    pub fn full_dump(&self) -> TransactionDump {
        let timestamp = Instant::now();

        // 트랜잭션 정보 수집
        let stats = self.tx_manager.stats();
        let active_transactions = self.collect_active_transactions();

        // 상태별 카운트
        let mut state_counts = HashMap::new();
        state_counts.insert("Active".to_string(), stats.active);
        state_counts.insert("Committed".to_string(), stats.committed);
        state_counts.insert("Aborted".to_string(), stats.aborted);

        TransactionDump {
            timestamp,
            active_transactions,
            total_count: stats.total,
            state_counts,
        }
    }

    /// 활성 트랜잭션 정보 수집
    fn collect_active_transactions(&self) -> Vec<TransactionInfo> {
        let active_ids = self.tx_manager.active_transactions();
        let mut result = Vec::new();

        // 대기 그래프에서 대기 정보 가져오기
        let wait_graph = self.lock_manager.get_wait_graph();
        let wait_map: HashMap<TransactionId, Vec<(LockResource, Vec<TransactionId>)>> = wait_graph
            .into_iter()
            .fold(HashMap::new(), |mut acc, (tx, res, holders)| {
                acc.entry(tx).or_default().push((res, holders));
                acc
            });

        for tx_id in active_ids {
            if let Some(ctx_arc) = self.tx_manager.get_transaction_context(tx_id) {
                let ctx = ctx_arc.read();
                let held_locks = self.lock_manager.locks_held_by(tx_id);
                let waiting_for = self.get_waiting_info_from_map(tx_id, &wait_map);

                result.push(TransactionInfo {
                    id: tx_id,
                    state: format!("{:?}", ctx.state),
                    held_locks,
                    waiting_for,
                    isolation_level: ctx.isolation_level,
                    started_at: ctx.start_time,
                    last_activity: ctx.last_activity,
                    statements_executed: ctx.statements_executed,
                    elapsed_ms: ctx.elapsed_ms(),
                });
            }
        }

        result
    }

    /// 대기 정보 조회 (대기 맵에서)
    fn get_waiting_info_from_map(
        &self,
        tx_id: TransactionId,
        wait_map: &HashMap<TransactionId, Vec<(LockResource, Vec<TransactionId>)>>,
    ) -> Option<LockWaitInfo> {
        wait_map.get(&tx_id).and_then(|waits| {
            waits.first().map(|(resource, holders)| {
                LockWaitInfo {
                    resource: format!("{:?}", resource),
                    requested_mode: "Unknown".to_string(),
                    holders: holders.clone(),
                    wait_started: Instant::now(), // 정확한 시작 시간은 추적하지 않음
                }
            })
        })
    }

    /// 트랜잭션 통계 수집 (Phase 4)
    ///
    /// LockManager에서 대기 시간 통계를 가져옵니다 (Issue #86).
    pub fn transaction_stats(&self) -> TransactionStats {
        let stats = self.tx_manager.stats();
        let active_transactions = self.collect_active_transactions();
        let lock_stats = self.lock_manager.get_lock_stats();

        let (total_duration, max_duration) = active_transactions
            .iter()
            .fold((0u64, 0u64), |(total, max), tx| {
                (total + tx.elapsed_ms, max.max(tx.elapsed_ms))
            });

        let avg_duration = if active_transactions.is_empty() {
            0.0
        } else {
            total_duration as f64 / active_transactions.len() as f64
        };

        // 총 대기 시간을 밀리초로 변환
        let lock_wait_time_ms = lock_stats.total_wait_time_ns / 1_000_000;

        TransactionStats {
            active_count: stats.active as u64,
            committed_count: stats.committed as u64,
            aborted_count: stats.aborted as u64,
            avg_duration_ms: avg_duration,
            max_duration_ms: max_duration,
            lock_wait_time_ms,
        }
    }

    /// 락 통계 수집 (Phase 4)
    ///
    /// LockManager에서 실제 통계를 가져옵니다 (Issue #86).
    pub fn lock_stats(&self) -> LockStats {
        let summary = self.lock_manager.lock_summary();
        let stats = self.lock_manager.get_lock_stats();

        LockStats {
            total_locks: summary.total_locks as u64,
            shared_locks: summary.shared_locks as u64,
            exclusive_locks: summary.exclusive_locks as u64,
            lock_conflicts: stats.lock_conflicts,
            deadlocks_detected: stats.deadlocks_detected,
            avg_wait_time_ms: stats.avg_wait_time_ms,
        }
    }

    /// 대기자 목록 조회 (Phase 2)
    pub fn get_waiters(&self) -> Vec<WaiterInfo> {
        let wait_graph = self.lock_manager.get_wait_graph();

        wait_graph
            .into_iter()
            .map(|(tx_id, _resource, _holders)| {
                WaiterInfo {
                    tx_id,
                    lock_mode: "Unknown".to_string(),
                    requested_at: Instant::now(),
                    wait_time_ms: 0, // 정확한 시작 시간 없음
                }
            })
            .collect()
    }

    /// 대기 중인 트랜잭션 ID 목록 조회 (Phase 2)
    pub fn get_waiting_transactions(&self) -> Vec<TransactionId> {
        let wait_graph = self.lock_manager.get_wait_graph();
        let mut waiting: HashSet<TransactionId> = HashSet::new();

        for (tx_id, _, _) in wait_graph {
            waiting.insert(tx_id);
        }

        waiting.into_iter().collect()
    }

    /// MVCC 상태 덤프
    pub fn mvcc_dump(&self) -> MvccDump {
        let stats = self.mvcc.stats();

        MvccDump {
            timestamp: Instant::now(),
            active_snapshots: stats.snapshots,
            total_snapshots_created: stats.current_timestamp, // 대략적 수치
            oldest_active_snapshot: stats.gc_watermark,
            versions_tracked: stats.commit_timestamps,
        }
    }
}

/// MVCC 덤프 정보
#[derive(Debug, Clone)]
pub struct MvccDump {
    /// 덤프 시간
    pub timestamp: Instant,
    /// 활성 스냅샷 수
    pub active_snapshots: usize,
    /// 총 생성된 스냅샷 수
    pub total_snapshots_created: u64,
    /// 가장 오래된 활성 스냅샷
    pub oldest_active_snapshot: Option<u64>,
    /// 추적 중인 버전 수
    pub versions_tracked: usize,
}

impl fmt::Display for MvccDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== MVCC State Dump ===")?;
        writeln!(f, "Timestamp: {:?}", self.timestamp)?;
        writeln!(f, "Active Snapshots: {}", self.active_snapshots)?;
        writeln!(
            f,
            "Total Snapshots Created: {}",
            self.total_snapshots_created
        )?;
        if let Some(oldest) = self.oldest_active_snapshot {
            writeln!(f, "Oldest Active Snapshot: {}", oldest)?;
        }
        writeln!(f, "Versions Tracked: {}", self.versions_tracked)?;
        Ok(())
    }
}

/// 락 체인 분석기
///
/// 락 대기 체인을 분석하여 데드락을 탐지합니다.
pub struct LockChainAnalyzer<'a> {
    lock_manager: &'a LockManager,
}

impl<'a> LockChainAnalyzer<'a> {
    /// 새 분석기 생성
    pub fn new(lock_manager: &'a LockManager) -> Self {
        Self { lock_manager }
    }

    /// 모든 대기 체인 분석
    pub fn analyze(&self) -> LockChainAnalysis {
        let start = Instant::now();

        // 대기 그래프 구성
        let wait_graph = self.build_wait_graph();

        // 체인 찾기
        let chains = self.find_all_chains(&wait_graph);

        // 통계 계산
        let deadlock_count = chains.iter().filter(|c| c.is_cycle).count();
        let max_chain_length = chains.iter().map(|c| c.length).max().unwrap_or(0);
        let waiting_transactions = wait_graph.len();

        LockChainAnalysis {
            chains,
            deadlock_count,
            max_chain_length,
            waiting_transactions,
            analysis_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// 대기 그래프 구성
    fn build_wait_graph(&self) -> HashMap<TransactionId, WaitNode> {
        let waits = self.lock_manager.get_wait_graph();
        let mut graph = HashMap::new();

        for (waiting_tx, resource, waiting_for_txs) in waits {
            graph.insert(
                waiting_tx,
                WaitNode {
                    resource: format!("{:?}", resource),
                    mode: "Unknown".to_string(),
                    waiting_for: waiting_for_txs,
                },
            );
        }

        graph
    }

    /// 모든 체인 찾기
    fn find_all_chains(&self, graph: &HashMap<TransactionId, WaitNode>) -> Vec<LockChain> {
        let mut chains = Vec::new();
        let mut visited_global = HashSet::new();

        for &start_tx in graph.keys() {
            if visited_global.contains(&start_tx) {
                continue;
            }

            let mut visited = HashSet::new();
            let mut path = Vec::new();

            if let Some(chain) = self.find_chain_from(start_tx, graph, &mut visited, &mut path) {
                visited_global.extend(visited);
                chains.push(chain);
            }
        }

        chains
    }

    /// 특정 트랜잭션에서 시작하는 체인 찾기
    fn find_chain_from(
        &self,
        tx_id: TransactionId,
        graph: &HashMap<TransactionId, WaitNode>,
        visited: &mut HashSet<TransactionId>,
        path: &mut Vec<LockChainNode>,
    ) -> Option<LockChain> {
        if visited.contains(&tx_id) {
            // 사이클 발견
            return Some(LockChain {
                start_tx: path.first().map(|n| n.tx_id).unwrap_or(tx_id),
                path: path.clone(),
                is_cycle: true,
                length: path.len(),
            });
        }

        visited.insert(tx_id);

        if let Some(node) = graph.get(&tx_id) {
            path.push(LockChainNode {
                tx_id,
                waiting_resource: Some(node.resource.clone()),
                requested_mode: Some(node.mode.clone()),
            });

            for &next_tx in &node.waiting_for {
                if let Some(chain) = self.find_chain_from(next_tx, graph, visited, path) {
                    return Some(chain);
                }
            }

            path.pop();
        } else if !path.is_empty() {
            // 체인 끝
            path.push(LockChainNode {
                tx_id,
                waiting_resource: None,
                requested_mode: None,
            });

            return Some(LockChain {
                start_tx: path.first().map(|n| n.tx_id).unwrap_or(tx_id),
                path: path.clone(),
                is_cycle: false,
                length: path.len(),
            });
        }

        None
    }

    /// 데드락만 찾기
    pub fn find_deadlocks(&self) -> Vec<LockChain> {
        self.analyze()
            .chains
            .into_iter()
            .filter(|c| c.is_cycle)
            .collect()
    }

    /// 가장 긴 대기 체인 찾기
    pub fn find_longest_chain(&self) -> Option<LockChain> {
        self.analyze().chains.into_iter().max_by_key(|c| c.length)
    }
}

/// 대기 노드
struct WaitNode {
    resource: String,
    mode: String,
    waiting_for: Vec<TransactionId>,
}

/// 통합 진단 도구
///
/// 트랜잭션, 락, MVCC 상태를 종합적으로 분석합니다.
pub struct TransactionDiagnostics<'a> {
    dumper: DiagnosticsDumper<'a>,
    lock_analyzer: LockChainAnalyzer<'a>,
}

impl<'a> TransactionDiagnostics<'a> {
    /// 새 진단 도구 생성
    pub fn new(
        tx_manager: &'a TransactionManager,
        lock_manager: &'a LockManager,
        mvcc: &'a MvccEngine,
    ) -> Self {
        Self {
            dumper: DiagnosticsDumper::new(tx_manager, lock_manager, mvcc),
            lock_analyzer: LockChainAnalyzer::new(lock_manager),
        }
    }

    /// 전체 시스템 진단
    pub fn full_diagnosis(&self) -> FullDiagnosis {
        let tx_dump = self.dumper.full_dump();
        let mvcc_dump = self.dumper.mvcc_dump();
        let lock_analysis = self.lock_analyzer.analyze();

        // 건강 상태 평가
        let health = self.evaluate_health(&tx_dump, &mvcc_dump, &lock_analysis);

        FullDiagnosis {
            timestamp: Instant::now(),
            transaction_dump: tx_dump,
            mvcc_dump,
            lock_analysis,
            health,
        }
    }

    /// 건강 상태 평가
    fn evaluate_health(
        &self,
        _tx_dump: &TransactionDump,
        mvcc_dump: &MvccDump,
        lock_analysis: &LockChainAnalysis,
    ) -> HealthStatus {
        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        // 데드락 체크
        if lock_analysis.deadlock_count > 0 {
            issues.push(format!(
                "{} deadlock(s) detected",
                lock_analysis.deadlock_count
            ));
        }

        // 긴 대기 체인 체크
        if lock_analysis.max_chain_length > 5 {
            warnings.push(format!(
                "Long lock chain detected (length: {})",
                lock_analysis.max_chain_length
            ));
        }

        // 많은 활성 스냅샷 체크
        if mvcc_dump.active_snapshots > 100 {
            warnings.push(format!(
                "High number of active snapshots ({})",
                mvcc_dump.active_snapshots
            ));
        }

        // 많은 대기 트랜잭션 체크
        if lock_analysis.waiting_transactions > 50 {
            warnings.push(format!(
                "High number of waiting transactions ({})",
                lock_analysis.waiting_transactions
            ));
        }

        let status = if !issues.is_empty() {
            "CRITICAL"
        } else if !warnings.is_empty() {
            "WARNING"
        } else {
            "HEALTHY"
        };

        HealthStatus {
            status: status.to_string(),
            issues,
            warnings,
        }
    }
}

/// 전체 진단 결과
#[derive(Debug, Clone)]
pub struct FullDiagnosis {
    /// 진단 시간
    pub timestamp: Instant,
    /// 트랜잭션 덤프
    pub transaction_dump: TransactionDump,
    /// MVCC 덤프
    pub mvcc_dump: MvccDump,
    /// 락 체인 분석
    pub lock_analysis: LockChainAnalysis,
    /// 건강 상태
    pub health: HealthStatus,
}

impl fmt::Display for FullDiagnosis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "╔══════════════════════════════════════════╗")?;
        writeln!(f, "║     TRANSACTION SYSTEM DIAGNOSIS         ║")?;
        writeln!(f, "╚══════════════════════════════════════════╝")?;
        writeln!(f)?;
        writeln!(f, "{}", self.health)?;
        writeln!(f)?;
        writeln!(f, "{}", self.transaction_dump)?;
        writeln!(f, "{}", self.mvcc_dump)?;
        writeln!(f, "{}", self.lock_analysis)?;
        Ok(())
    }
}

/// 건강 상태
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// 상태 (HEALTHY, WARNING, CRITICAL)
    pub status: String,
    /// 심각한 이슈
    pub issues: Vec<String>,
    /// 경고
    pub warnings: Vec<String>,
}

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = match self.status.as_str() {
            "HEALTHY" => "✓",
            "WARNING" => "⚠",
            "CRITICAL" => "✗",
            _ => "?",
        };

        writeln!(f, "Health Status: {} {}", icon, self.status)?;

        if !self.issues.is_empty() {
            writeln!(f, "Issues:")?;
            for issue in &self.issues {
                writeln!(f, "  ✗ {}", issue)?;
            }
        }

        if !self.warnings.is_empty() {
            writeln!(f, "Warnings:")?;
            for warning in &self.warnings {
                writeln!(f, "  ⚠ {}", warning)?;
            }
        }

        Ok(())
    }
}
