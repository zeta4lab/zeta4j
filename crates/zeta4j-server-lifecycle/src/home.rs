//! Home-directory and PID-lock utilities.

use fs4::FileExt;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::process::is_process_running;

/// Existing PID-file lock ownership observed without modifying the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidLockState {
    Missing,
    Unlocked,
    Locked,
}

/// 홈 디렉토리 결정
pub fn get_home_dir(home: Option<PathBuf>) -> PathBuf {
    home.or_else(|| env::var("ZETA4J_HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".zeta4j"))
                .unwrap_or_else(|| PathBuf::from(".zeta4j"))
        })
}

/// PID 파일 경로
pub fn get_pid_path(home: &Path) -> PathBuf {
    home.join("zeta4j.pid")
}

/// 로그 파일 경로
pub fn get_log_path(home: &Path) -> PathBuf {
    home.join("logs").join("zeta4j.log")
}

/// PID 파일 읽기
pub fn read_pid_file(home: &Path) -> Option<u32> {
    let pid_path = get_pid_path(home);
    if !pid_path.exists() {
        return None;
    }

    fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// PID 파일 쓰기
pub fn write_pid_file(home: &Path, pid: u32) -> std::io::Result<()> {
    let pid_path = get_pid_path(home);
    fs::write(&pid_path, pid.to_string())
}

/// PID 파일 삭제
pub fn remove_pid_file(home: &Path) -> std::io::Result<()> {
    let pid_path = get_pid_path(home);
    if pid_path.exists() {
        fs::remove_file(&pid_path)?;
    }
    Ok(())
}

/// Inspect whether an existing PID file is locked without truncating or creating it.
pub fn inspect_pid_lock(home: &Path) -> io::Result<PidLockState> {
    let pid_path = get_pid_path(home);
    let file = match OpenOptions::new().read(true).write(true).open(&pid_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PidLockState::Missing);
        }
        Err(error) => return Err(error),
    };

    match file.try_lock_exclusive() {
        Ok(()) => {
            file.unlock()?;
            Ok(PidLockState::Unlocked)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(PidLockState::Locked),
        Err(error) => Err(error),
    }
}

/// Remove an existing PID file only while holding its exclusive lock.
///
/// Returns `Ok(false)` when the file does not exist or another process owns the lock.
pub fn remove_pid_file_if_unlocked(home: &Path) -> io::Result<bool> {
    let pid_path = get_pid_path(home);
    let file = match OpenOptions::new().read(true).write(true).open(&pid_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    match file.try_lock_exclusive() {
        Ok(()) => {
            fs::remove_file(&pid_path)?;
            file.unlock()?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error),
    }
}

// =============================================================================
// PID 파일 잠금 (flock)
// =============================================================================

/// PID 파일 잠금 구조체
///
/// 파일 잠금을 통해 동시 다중 인스턴스 실행을 방지합니다.
/// RAII 패턴으로 Drop 시 자동으로 잠금 해제 및 파일 삭제됩니다.
#[derive(Debug)]
pub struct PidLock {
    file: File,
    path: PathBuf,
}

impl PidLock {
    /// PID 파일 잠금 획득 및 PID 기록
    ///
    /// # Arguments
    /// * `home` - 홈 디렉토리 경로
    /// * `pid` - 기록할 프로세스 ID
    ///
    /// # Returns
    /// * `Ok(PidLock)` - 잠금 획득 성공
    /// * `Err(io::Error)` - 잠금 획득 실패 (다른 인스턴스 실행 중)
    pub fn acquire(home: &Path, pid: u32) -> io::Result<Self> {
        let path = get_pid_path(home);

        // 디렉토리 생성
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        // 배타적 잠금 시도 (논블로킹)
        file.try_lock_exclusive().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "Another instance is already running (lock held)",
            )
        })?;

        // 락을 소유한 뒤에만 기존 내용을 교체한다. 락 획득 전 truncate하면
        // 다른 인스턴스가 사용 중인 PID 파일을 훼손할 수 있다.
        let mut file = file;
        file.set_len(0)?;
        writeln!(file, "{}", pid)?;
        file.sync_all()?;

        Ok(Self { file, path })
    }

    /// PID 파일 경로 반환
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        // 다른 인스턴스가 같은 경로를 획득하기 전에 락을 보유한 채 삭제한다.
        let _ = fs::remove_file(&self.path);
        let _ = self.file.unlock();
    }
}

/// Stale PID 파일 정리
///
/// PID 파일이 존재하지만 해당 프로세스가 실행 중이 아닌 경우 파일을 삭제합니다.
///
/// # Arguments
/// * `home` - 홈 디렉토리 경로
///
/// # Returns
/// * `Ok(true)` - Stale PID 파일이 정리됨
/// * `Ok(false)` - 정리할 파일 없음 (파일 없거나 프로세스 실행 중)
/// * `Err` - I/O 에러
pub fn cleanup_stale_pid(home: &Path) -> io::Result<bool> {
    let pid_path = get_pid_path(home);

    // PID 파일이 없으면 정리할 것 없음
    if !pid_path.exists() {
        return Ok(false);
    }

    // PID 읽기
    let pid = match fs::read_to_string(&pid_path) {
        Ok(content) => match content.trim().parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => {
                return remove_pid_file_if_unlocked(home);
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    // 프로세스 실행 여부 확인
    if !is_process_running(pid) {
        return remove_pid_file_if_unlocked(home);
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{
        PidLock, PidLockState, cleanup_stale_pid, inspect_pid_lock, remove_pid_file_if_unlocked,
    };

    #[test]
    fn inspects_missing_unlocked_and_locked_pid_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        assert_eq!(inspect_pid_lock(temp.path())?, PidLockState::Missing);

        std::fs::write(temp.path().join("zeta4j.pid"), "1234\n")?;
        assert_eq!(inspect_pid_lock(temp.path())?, PidLockState::Unlocked);

        let lock = PidLock::acquire(temp.path(), std::process::id())?;
        assert_eq!(inspect_pid_lock(temp.path())?, PidLockState::Locked);
        drop(lock);
        assert_eq!(inspect_pid_lock(temp.path())?, PidLockState::Missing);
        Ok(())
    }

    #[test]
    fn inspection_does_not_change_pid_contents() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let pid_path = temp.path().join("zeta4j.pid");
        std::fs::write(&pid_path, "not-a-pid\n")?;

        assert_eq!(inspect_pid_lock(temp.path())?, PidLockState::Unlocked);
        assert_eq!(std::fs::read_to_string(pid_path)?, "not-a-pid\n");
        Ok(())
    }

    #[test]
    fn removes_only_an_unlocked_pid_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let pid_path = temp.path().join("zeta4j.pid");
        std::fs::write(&pid_path, "1234\n")?;
        assert!(remove_pid_file_if_unlocked(temp.path())?);
        assert!(!pid_path.exists());

        let lock = PidLock::acquire(temp.path(), std::process::id())?;
        assert!(!remove_pid_file_if_unlocked(temp.path())?);
        assert!(pid_path.exists());
        drop(lock);
        Ok(())
    }

    #[test]
    fn competing_acquire_preserves_the_owned_pid_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let pid_path = temp.path().join("zeta4j.pid");
        let lock = PidLock::acquire(temp.path(), 1234)?;

        let error = PidLock::acquire(temp.path(), 5678)
            .err()
            .ok_or("competing lock acquisition unexpectedly succeeded")?;
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(std::fs::read_to_string(&pid_path)?, "1234\n");
        assert!(!cleanup_stale_pid(temp.path())?);
        assert_eq!(std::fs::read_to_string(&pid_path)?, "1234\n");

        drop(lock);
        Ok(())
    }
}
