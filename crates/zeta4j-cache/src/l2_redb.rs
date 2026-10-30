//! L2 캐시 (redb 기반 디스크 영속 저장소)

use crate::error::{Result, ZetaError};
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use std::sync::Arc;

/// L2 캐시 래퍼
pub struct L2Cache<K, V> {
    db: Arc<Database>,
    table_def: TableDefinition<'static, &'static [u8], &'static [u8]>,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> L2Cache<K, V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    /// 기존 redb::Database에서 테이블을 열어 L2 캐시 생성
    ///
    /// 여러 L2Cache가 같은 redb::Database를 공유할 때 사용합니다.
    pub fn with_db(db: Arc<Database>, tree_name: &'static str) -> Result<Self> {
        let table_def = TableDefinition::new(tree_name);

        // Write 트랜잭션을 열어 테이블 생성 보장
        let tx = db
            .begin_write()
            .map_err(|e| ZetaError::cache_init(e.to_string()))?;
        tx.open_table(table_def)
            .map_err(|e| ZetaError::cache_init(e.to_string()))?;
        tx.commit()
            .map_err(|e| ZetaError::cache_init(e.to_string()))?;

        Ok(Self {
            db,
            table_def,
            _phantom: PhantomData,
        })
    }

    /// 값 조회
    pub fn get(&self, key: &K) -> Result<Option<V>> {
        let key_bytes = zeta4j_codec::serialize(key)
            .map_err(|e| ZetaError::cache_serialization(e.to_string()))?;

        let tx = self
            .db
            .begin_read()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        let table = tx
            .open_table(self.table_def)
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;

        match table
            .get(key_bytes.as_slice())
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?
        {
            Some(access) => {
                let value: V = zeta4j_codec::deserialize(access.value())
                    .map_err(|e| ZetaError::cache_serialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// 값 삽입
    pub fn insert(&self, key: &K, value: &V) -> Result<()> {
        let key_bytes = zeta4j_codec::serialize(key)
            .map_err(|e| ZetaError::cache_serialization(e.to_string()))?;
        let value_bytes = zeta4j_codec::serialize(value)
            .map_err(|e| ZetaError::cache_serialization(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;

        {
            let mut table = tx
                .open_table(self.table_def)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
            table
                .insert(key_bytes.as_slice(), value_bytes.as_slice())
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        Ok(())
    }

    /// 값 무효화
    pub fn invalidate(&self, key: &K) -> Result<()> {
        let key_bytes = zeta4j_codec::serialize(key)
            .map_err(|e| ZetaError::cache_serialization(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;

        {
            let mut table = tx
                .open_table(self.table_def)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
            table
                .remove(key_bytes.as_slice())
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        Ok(())
    }

    /// 특정 접두사로 시작하는 모든 키 무효화
    pub fn invalidate_by_prefix(&self, prefix: &[u8]) -> Result<u64> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        let mut count = 0;

        {
            let mut table = tx
                .open_table(self.table_def)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
            // redb는 range()를 지원합니다. 접두사에 해당하는 범위를 삭제합니다.
            let mut keys_to_remove = Vec::new();

            for result in table
                .range::<&[u8]>(prefix..)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?
            {
                let (key_access, _) =
                    result.map_err(|e| ZetaError::cache_storage(e.to_string()))?;
                let key = key_access.value();
                if key.starts_with(prefix) {
                    keys_to_remove.push(key.to_vec());
                } else {
                    break; // Because the keys are ordered, if it doesn't start with prefix anymore, we can stop
                }
            }

            for key in keys_to_remove {
                table
                    .remove(key.as_slice())
                    .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        Ok(count)
    }

    /// 모든 값 무효화
    pub fn clear(&self) -> Result<()> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        {
            let mut table = tx
                .open_table(self.table_def)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
            // retain() with false clears the table in redb
            table
                .retain_in::<&[u8], _>(.., |_, _| false)
                .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        Ok(())
    }

    /// 현재 항목 수
    pub fn len(&self) -> Result<usize> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        let table = tx
            .open_table(self.table_def)
            .map_err(|e| ZetaError::cache_storage(e.to_string()))?;
        table
            .len()
            .map(|len| len as usize)
            .map_err(|e| ZetaError::cache_storage(e.to_string()))
    }
}
