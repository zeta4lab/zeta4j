// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server host system-information utilities.

use std::process::Command;

#[cfg(target_os = "linux")]
use std::fs;

use super::home::get_home_dir;

/// 디스크 여유 공간 가져오기 (MB)
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn get_disk_space_mb() -> Option<u64> {
    use std::mem::MaybeUninit;

    let home = get_home_dir(None);
    let path_cstr = std::ffi::CString::new(home.to_string_lossy().as_ref()).ok()?;

    unsafe {
        let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
        if libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) == 0 {
            let stat = stat.assume_init();
            let free_bytes = u128::from(stat.f_bavail) * u128::from(stat.f_frsize);
            u64::try_from(free_bytes / (1024 * 1024)).ok()
        } else {
            None
        }
    }
}

#[cfg(windows)]
pub fn get_disk_space_mb() -> Option<u64> {
    // Windows에서는 GetDiskFreeSpaceExW 사용 필요
    // 간단히 None 반환
    None
}

/// 여유 메모리 가져오기 (MB)
#[cfg(target_os = "macos")]
pub fn get_free_memory_mb() -> Option<u64> {
    let output = Command::new("vm_stat").output().ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut free_pages = 0u64;
        let mut inactive_pages = 0u64;

        for line in stdout.lines() {
            if line.contains("Pages free:") {
                if let Some(num_str) = line.split(':').nth(1) {
                    free_pages = num_str.trim().trim_end_matches('.').parse().unwrap_or(0);
                }
            } else if line.contains("Pages inactive:")
                && let Some(num_str) = line.split(':').nth(1)
            {
                inactive_pages = num_str.trim().trim_end_matches('.').parse().unwrap_or(0);
            }
        }

        // 페이지 크기는 보통 4096 바이트
        let page_size = 4096u64;
        let free_bytes = (free_pages + inactive_pages) * page_size;
        Some(free_bytes / (1024 * 1024))
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub fn get_free_memory_mb() -> Option<u64> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;

    for line in content.lines() {
        if line.starts_with("MemAvailable:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                return Some(kb / 1024);
            }
        }
    }

    None
}

#[cfg(windows)]
pub fn get_free_memory_mb() -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn get_free_memory_mb() -> Option<u64> {
    None
}

/// ulimit -n 값 가져오기
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn get_ulimit_nofile() -> Option<u64> {
    use std::mem::MaybeUninit;

    unsafe {
        let mut rlim: MaybeUninit<libc::rlimit> = MaybeUninit::uninit();
        if libc::getrlimit(libc::RLIMIT_NOFILE, rlim.as_mut_ptr()) == 0 {
            let rlim = rlim.assume_init();
            Some(rlim.rlim_cur)
        } else {
            None
        }
    }
}

#[cfg(windows)]
pub fn get_ulimit_nofile() -> Option<u64> {
    None
}

/// 시스템 정보 수집 (진단용)
pub fn collect_system_info() -> String {
    let mut info = String::new();

    info.push_str("=== System Information ===\n");
    info.push_str(&format!(
        "Date: {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    info.push_str(&format!("OS: {}\n", std::env::consts::OS));
    info.push_str(&format!("Arch: {}\n", std::env::consts::ARCH));

    #[cfg(unix)]
    {
        // uname 정보
        if let Ok(output) = Command::new("uname").args(["-a"]).output()
            && output.status.success()
        {
            info.push_str(&format!(
                "uname: {}\n",
                String::from_utf8_lossy(&output.stdout).trim()
            ));
        }
    }

    // 메모리 정보
    if let Some(mb) = get_free_memory_mb() {
        info.push_str(&format!("Free Memory: {} MB\n", mb));
    }

    // 디스크 정보
    if let Some(mb) = get_disk_space_mb() {
        info.push_str(&format!("Free Disk: {} MB\n", mb));
    }

    #[cfg(unix)]
    {
        // ulimit 정보
        if let Some(limit) = get_ulimit_nofile() {
            info.push_str(&format!("File Descriptor Limit: {}\n", limit));
        }
    }

    info
}
