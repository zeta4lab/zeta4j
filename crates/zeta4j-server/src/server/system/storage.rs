//! System Storage (Redb wrapper)
//!
//! 시스템 메타데이터를 위한 영속 스토리지 (Phase 2)
//!
//! ## Tables
//! - `catalog`: 데이터베이스 카탈로그
//! - `users`: 사용자 정보
//! - `indexes`: 인덱스 정의
//! - `constraints`: 제약조건 정의
//! - `settings`: 시스템 설정
//! - `meta`: 메타 정보 (스키마 버전 등)

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use super::error::{SystemError, SystemResult};

const SECURITY_SCHEMA_KEY: &[u8] = b"security-schema-version";
const SECURITY_SCHEMA_VERSION: u32 = 4;
type SystemTable = TableDefinition<'static, &'static [u8], &'static [u8]>;

const CATALOG_TABLE: SystemTable = TableDefinition::new(cf::CATALOG);
const USERS_TABLE: SystemTable = TableDefinition::new(cf::USERS);
const INDEXES_TABLE: SystemTable = TableDefinition::new(cf::INDEXES);
const CONSTRAINTS_TABLE: SystemTable = TableDefinition::new(cf::CONSTRAINTS);
const SETTINGS_TABLE: SystemTable = TableDefinition::new(cf::SETTINGS);
const META_TABLE: SystemTable = TableDefinition::new(cf::META);
const PRINCIPALS_TABLE: SystemTable = TableDefinition::new(cf::PRINCIPALS);
const SESSIONS_TABLE: SystemTable = TableDefinition::new(cf::SESSIONS);
const REFRESH_FAMILIES_TABLE: SystemTable = TableDefinition::new(cf::REFRESH_FAMILIES);
const API_KEYS_TABLE: SystemTable = TableDefinition::new(cf::API_KEYS);
const SIGNING_KEYS_TABLE: SystemTable = TableDefinition::new(cf::SIGNING_KEYS);
const CREDENTIAL_THROTTLES_TABLE: SystemTable = TableDefinition::new(cf::CREDENTIAL_THROTTLES);
const SERVICE_PRINCIPALS_TABLE: SystemTable = TableDefinition::new(cf::SERVICE_PRINCIPALS);

/// Column Family (Table) 이름 상수
pub mod cf {
    pub const CATALOG: &str = "catalog";
    pub const USERS: &str = "users";
    pub const INDEXES: &str = "indexes";
    pub const CONSTRAINTS: &str = "constraints";
    pub const SETTINGS: &str = "settings";
    pub const META: &str = "meta";
    pub const PRINCIPALS: &str = "security_principals";
    pub const SESSIONS: &str = "security_sessions";
    pub const REFRESH_FAMILIES: &str = "security_refresh_families";
    pub const API_KEYS: &str = "security_api_keys";
    pub const SIGNING_KEYS: &str = "security_signing_keys";
    pub const CREDENTIAL_THROTTLES: &str = "security_credential_throttles";
    pub const SERVICE_PRINCIPALS: &str = "security_service_principals";
}

/// 시스템 스토리지 (redb 기반)
pub struct SystemStorage {
    db: Arc<Database>,
    path: std::path::PathBuf,
}

impl SystemStorage {
    /// 스토리지 열기 (없으면 생성)
    pub fn open(path: impl AsRef<Path>) -> SystemResult<Self> {
        let path = path.as_ref();
        info!("Opening system storage at: {}", path.display());

        std::fs::create_dir_all(path).map_err(|error| {
            SystemError::storage(format!("system storage path rejected: {error}"))
        })?;

        let db_path = path.join("system.redb");
        let db =
            Arc::new(Database::create(&db_path).map_err(|e| SystemError::storage(e.to_string()))?);

        let tables = [
            CATALOG_TABLE,
            USERS_TABLE,
            INDEXES_TABLE,
            CONSTRAINTS_TABLE,
            SETTINGS_TABLE,
            META_TABLE,
            PRINCIPALS_TABLE,
            SESSIONS_TABLE,
            REFRESH_FAMILIES_TABLE,
            API_KEYS_TABLE,
            SIGNING_KEYS_TABLE,
            CREDENTIAL_THROTTLES_TABLE,
            SERVICE_PRINCIPALS_TABLE,
        ];

        // 초기 시작 시 테이블이 생성되어 있도록 보장
        {
            let tx = db
                .begin_write()
                .map_err(|e| SystemError::storage(e.to_string()))?;
            for table in tables {
                tx.open_table(table)
                    .map_err(|e| SystemError::storage(e.to_string()))?;
            }
            tx.commit()
                .map_err(|e| SystemError::storage(e.to_string()))?;
        }

        let storage = Self {
            db,
            path: path.to_path_buf(),
        };
        storage.validate_security_schema()?;
        info!("System storage opened successfully");
        Ok(storage)
    }

    fn validate_security_schema(&self) -> SystemResult<()> {
        match self.get::<u32>(cf::META, SECURITY_SCHEMA_KEY)? {
            Some(SECURITY_SCHEMA_VERSION) => Ok(()),
            Some(version) => Err(SystemError::InvalidConfiguration(format!(
                "[SV-AUTH-006] security state schema {version} is incompatible with required schema {SECURITY_SCHEMA_VERSION}; perform a clean reinitialization"
            ))),
            None if self.table_is_empty(USERS_TABLE)? => {
                self.put(
                    cf::META,
                    SECURITY_SCHEMA_KEY,
                    &SECURITY_SCHEMA_VERSION,
                )
            }
            None => Err(SystemError::InvalidConfiguration(
                "[SV-AUTH-006] security state has no schema manifest; perform a clean reinitialization"
                    .to_string(),
            )),
        }
    }

    fn table_is_empty(&self, definition: SystemTable) -> SystemResult<bool> {
        let transaction = self
            .db
            .begin_read()
            .map_err(|error| SystemError::storage(error.to_string()))?;
        let table = transaction
            .open_table(definition)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        table
            .is_empty()
            .map_err(|error| SystemError::storage(error.to_string()))
    }

    fn table_def(name: &str) -> SystemResult<SystemTable> {
        match name {
            cf::CATALOG => Ok(CATALOG_TABLE),
            cf::USERS => Ok(USERS_TABLE),
            cf::INDEXES => Ok(INDEXES_TABLE),
            cf::CONSTRAINTS => Ok(CONSTRAINTS_TABLE),
            cf::SETTINGS => Ok(SETTINGS_TABLE),
            cf::META => Ok(META_TABLE),
            cf::PRINCIPALS => Ok(PRINCIPALS_TABLE),
            cf::SESSIONS => Ok(SESSIONS_TABLE),
            cf::REFRESH_FAMILIES => Ok(REFRESH_FAMILIES_TABLE),
            cf::API_KEYS => Ok(API_KEYS_TABLE),
            cf::SIGNING_KEYS => Ok(SIGNING_KEYS_TABLE),
            cf::CREDENTIAL_THROTTLES => Ok(CREDENTIAL_THROTTLES_TABLE),
            cf::SERVICE_PRINCIPALS => Ok(SERVICE_PRINCIPALS_TABLE),
            _ => Err(SystemError::storage(format!(
                "Unknown system table: {name}"
            ))),
        }
    }

    /// 값 가져오기
    pub fn get<T: DeserializeOwned>(&self, cf_name: &str, key: &[u8]) -> SystemResult<Option<T>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        match table
            .get(key)
            .map_err(|e| SystemError::storage(e.to_string()))?
        {
            Some(access) => {
                let value: T = zeta4j_codec::deserialize(access.value())
                    .map_err(|e| SystemError::deserialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// 값 저장
    pub fn put<T: Serialize>(&self, cf_name: &str, key: &[u8], value: &T) -> SystemResult<()> {
        let bytes = zeta4j_codec::serialize(value)
            .map_err(|e| SystemError::serialization(e.to_string()))?;
        let tx = self
            .db
            .begin_write()
            .map_err(|e| SystemError::storage(e.to_string()))?;

        {
            let table_def = Self::table_def(cf_name)?;
            let mut table = tx
                .open_table(table_def)
                .map_err(|e| SystemError::storage(e.to_string()))?;
            table
                .insert(key, bytes.as_slice())
                .map_err(|e| SystemError::storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        Ok(())
    }

    /// 값 삭제
    pub fn delete(&self, cf_name: &str, key: &[u8]) -> SystemResult<()> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| SystemError::storage(e.to_string()))?;

        {
            let table_def = Self::table_def(cf_name)?;
            let mut table = tx
                .open_table(table_def)
                .map_err(|e| SystemError::storage(e.to_string()))?;
            table
                .remove(key)
                .map_err(|e| SystemError::storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        Ok(())
    }

    /// 키 존재 여부 확인
    pub fn exists(&self, cf_name: &str, key: &[u8]) -> SystemResult<bool> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        Ok(table
            .get(key)
            .map_err(|e| SystemError::storage(e.to_string()))?
            .is_some())
    }

    /// 프리픽스로 스캔
    pub fn scan_prefix<T: DeserializeOwned>(
        &self,
        cf_name: &str,
        prefix: &[u8],
    ) -> SystemResult<Vec<(Vec<u8>, T)>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        let mut results = Vec::new();

        // iterate with range
        for result in table
            .range::<&[u8]>(prefix..)
            .map_err(|e| SystemError::storage(e.to_string()))?
        {
            let (k_access, v_access) = result.map_err(|e| SystemError::storage(e.to_string()))?;
            let key = k_access.value();

            if !key.starts_with(prefix) {
                break;
            }

            let decoded: T = zeta4j_codec::deserialize(v_access.value())
                .map_err(|e| SystemError::deserialization(e.to_string()))?;
            results.push((key.to_vec(), decoded));
        }

        Ok(results)
    }

    /// 전체 스캔
    pub fn scan_all<T: DeserializeOwned>(&self, cf_name: &str) -> SystemResult<Vec<(Vec<u8>, T)>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        let mut results = Vec::new();

        // full range scan
        for result in table
            .range::<&[u8]>(..)
            .map_err(|e| SystemError::storage(e.to_string()))?
        {
            let (k_access, v_access) = result.map_err(|e| SystemError::storage(e.to_string()))?;
            let decoded: T = zeta4j_codec::deserialize(v_access.value())
                .map_err(|e| SystemError::deserialization(e.to_string()))?;
            results.push((k_access.value().to_vec(), decoded));
        }

        Ok(results)
    }

    /// 배치 쓰기
    pub fn write_batch(&self, batch: SystemWriteBatch) -> SystemResult<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let tx = self
            .db
            .begin_write()
            .map_err(|e| SystemError::storage(e.to_string()))?;

        for op in batch.ops {
            match op {
                WriteOp::Put {
                    cf_name,
                    key,
                    value,
                } => {
                    let table_def = Self::table_def(&cf_name)?;
                    let mut table = tx
                        .open_table(table_def)
                        .map_err(|e| SystemError::storage(e.to_string()))?;
                    table
                        .insert(key.as_slice(), value.as_slice())
                        .map_err(|e| SystemError::storage(e.to_string()))?;
                }
                WriteOp::Delete { cf_name, key } => {
                    let table_def = Self::table_def(&cf_name)?;
                    let mut table = tx
                        .open_table(table_def)
                        .map_err(|e| SystemError::storage(e.to_string()))?;
                    table
                        .remove(key.as_slice())
                        .map_err(|e| SystemError::storage(e.to_string()))?;
                }
            }
        }

        tx.commit()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        Ok(())
    }

    /// Run reads and writes against one redb write transaction.
    ///
    /// Returning an error aborts the transaction without committing any write.
    pub fn write_transaction<R>(
        &self,
        operation: impl FnOnce(&SystemWriteTransaction<'_>) -> SystemResult<R>,
    ) -> SystemResult<R> {
        let transaction = self
            .db
            .begin_write()
            .map_err(|error| SystemError::storage(error.to_string()))?;
        let result = {
            let transaction_view = SystemWriteTransaction {
                transaction: &transaction,
            };
            operation(&transaction_view)
        };
        match result {
            Ok(value) => {
                transaction
                    .commit()
                    .map_err(|error| SystemError::storage(error.to_string()))?;
                Ok(value)
            }
            Err(error) => {
                transaction
                    .abort()
                    .map_err(|abort_error| SystemError::storage(abort_error.to_string()))?;
                Err(error)
            }
        }
    }

    /// 새 배치 생성
    pub fn batch(&self) -> SystemWriteBatch {
        SystemWriteBatch::new()
    }

    /// 스토리지 경로
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ========================================================================
    // Raw 바이트 메서드 (직렬화 없이 직접 저장)
    // ========================================================================

    pub fn put_raw(&self, cf_name: &str, key: &[u8], value: &[u8]) -> SystemResult<()> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| SystemError::storage(e.to_string()))?;

        {
            let table_def = Self::table_def(cf_name)?;
            let mut table = tx
                .open_table(table_def)
                .map_err(|e| SystemError::storage(e.to_string()))?;
            table
                .insert(key, value)
                .map_err(|e| SystemError::storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_raw(&self, cf_name: &str, key: &[u8]) -> SystemResult<Option<Vec<u8>>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        match table
            .get(key)
            .map_err(|e| SystemError::storage(e.to_string()))?
        {
            Some(access) => Ok(Some(access.value().to_vec())),
            None => Ok(None),
        }
    }

    pub fn scan_all_raw(&self, cf_name: &str) -> SystemResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| SystemError::storage(e.to_string()))?;
        let table_def = Self::table_def(cf_name)?;
        let table = tx
            .open_table(table_def)
            .map_err(|e| SystemError::storage(e.to_string()))?;

        let mut results = Vec::new();

        for result in table
            .range::<&[u8]>(..)
            .map_err(|e| SystemError::storage(e.to_string()))?
        {
            let (k_access, v_access) = result.map_err(|e| SystemError::storage(e.to_string()))?;
            results.push((k_access.value().to_vec(), v_access.value().to_vec()));
        }

        Ok(results)
    }
}

/// Typed access to a single redb write transaction.
pub struct SystemWriteTransaction<'transaction> {
    transaction: &'transaction WriteTransaction,
}

impl SystemWriteTransaction<'_> {
    pub fn get<T: DeserializeOwned>(&self, cf_name: &str, key: &[u8]) -> SystemResult<Option<T>> {
        let table_definition = SystemStorage::table_def(cf_name)?;
        let table = self
            .transaction
            .open_table(table_definition)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        let value = table
            .get(key)
            .map_err(|error| SystemError::storage(error.to_string()))?
            .map(|access| access.value().to_vec());
        value
            .map(|bytes| {
                zeta4j_codec::deserialize(&bytes)
                    .map_err(|error| SystemError::deserialization(error.to_string()))
            })
            .transpose()
    }

    pub fn put<T: Serialize>(&self, cf_name: &str, key: &[u8], value: &T) -> SystemResult<()> {
        let bytes = zeta4j_codec::serialize(value)
            .map_err(|error| SystemError::serialization(error.to_string()))?;
        let table_definition = SystemStorage::table_def(cf_name)?;
        let mut table = self
            .transaction
            .open_table(table_definition)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        table
            .insert(key, bytes.as_slice())
            .map_err(|error| SystemError::storage(error.to_string()))?;
        Ok(())
    }

    pub fn delete(&self, cf_name: &str, key: &[u8]) -> SystemResult<()> {
        let table_definition = SystemStorage::table_def(cf_name)?;
        let mut table = self
            .transaction
            .open_table(table_definition)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        table
            .remove(key)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        Ok(())
    }

    pub fn scan_prefix<T: DeserializeOwned>(
        &self,
        cf_name: &str,
        prefix: &[u8],
    ) -> SystemResult<Vec<(Vec<u8>, T)>> {
        let table_definition = SystemStorage::table_def(cf_name)?;
        let table = self
            .transaction
            .open_table(table_definition)
            .map_err(|error| SystemError::storage(error.to_string()))?;
        let mut values = Vec::new();
        for row in table
            .range::<&[u8]>(prefix..)
            .map_err(|error| SystemError::storage(error.to_string()))?
        {
            let (key, value) = row.map_err(|error| SystemError::storage(error.to_string()))?;
            if !key.value().starts_with(prefix) {
                break;
            }
            let decoded = zeta4j_codec::deserialize(value.value())
                .map_err(|error| SystemError::deserialization(error.to_string()))?;
            values.push((key.value().to_vec(), decoded));
        }
        Ok(values)
    }
}

/// 인메모리 버퍼 큐에 저장될 개별 쓰기 오퍼레이션
pub enum WriteOp {
    Put {
        cf_name: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf_name: String,
        key: Vec<u8>,
    },
}

/// 배치 쓰기 래퍼 (Redb 인메모리 버퍼 구현)
pub struct SystemWriteBatch {
    ops: Vec<WriteOp>,
}

impl SystemWriteBatch {
    /// 새 배치 생성
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// 값 추가
    pub fn put<T: Serialize>(&mut self, cf_name: &str, key: &[u8], value: &T) -> SystemResult<()> {
        let bytes = zeta4j_codec::serialize(value)
            .map_err(|e| SystemError::serialization(e.to_string()))?;
        self.ops.push(WriteOp::Put {
            cf_name: cf_name.to_string(),
            key: key.to_vec(),
            value: bytes,
        });
        Ok(())
    }

    /// 값 삭제
    pub fn delete(&mut self, cf_name: &str, key: &[u8]) -> SystemResult<()> {
        self.ops.push(WriteOp::Delete {
            cf_name: cf_name.to_string(),
            key: key.to_vec(),
        });
        Ok(())
    }

    /// 배치 크기
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// 배치가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl Default for SystemWriteBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Arc로 감싼 스토리지
pub type SharedSystemStorage = Arc<SystemStorage>;

#[cfg(test)]
mod tests {
    use super::{SystemError, SystemStorage, cf};

    #[test]
    fn directory_path_creates_reopenable_redb_file() -> Result<(), Box<dyn std::error::Error>> {
        let dir = crate::server::system::owner_only_tempdir()?;

        {
            let storage = SystemStorage::open(dir.path())?;
            storage.put(cf::SETTINGS, b"answer", &42_u64)?;
        }

        assert!(dir.path().join("system.redb").is_file());

        let reopened = SystemStorage::open(dir.path())?;
        assert_eq!(reopened.get::<u64>(cf::SETTINGS, b"answer")?, Some(42));
        Ok(())
    }

    #[test]
    fn unknown_table_name_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = crate::server::system::owner_only_tempdir()?;
        let storage = SystemStorage::open(dir.path())?;

        let result = storage.put_raw("settings_typo", b"key", b"value");

        let Err(error) = result else {
            return Err(std::io::Error::other("unknown table was created").into());
        };
        let message = error.to_string();
        assert!(message.contains("Unknown system table"));
        Ok(())
    }

    #[test]
    fn failed_write_transaction_does_not_commit_partial_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = crate::server::system::owner_only_tempdir()?;
        let storage = SystemStorage::open(dir.path())?;

        let result: Result<(), SystemError> = storage.write_transaction(|transaction| {
            transaction.put(cf::SETTINGS, b"partial", &42_u64)?;
            Err(SystemError::InvalidConfiguration(
                "injected failure".to_string(),
            ))
        });

        assert!(result.is_err());
        assert_eq!(storage.get::<u64>(cf::SETTINGS, b"partial")?, None);
        Ok(())
    }
}
