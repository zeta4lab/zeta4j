// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server process inspection and termination utilities.

use std::process::Command;

/// 프로세스가 실행 중인지 확인
#[cfg(unix)]
pub fn is_process_running(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn is_process_running(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// 프로세스 종료
#[cfg(unix)]
pub fn kill_process(pid: u32, force: bool) -> bool {
    let signal = if force { "KILL" } else { "TERM" };
    Command::new("kill")
        .args([&format!("-{}", signal), &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn kill_process(pid: u32, _force: bool) -> bool {
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 특정 포트를 사용 중인 프로세스 PID 찾기
#[cfg(unix)]
pub fn find_process_using_port(port: u16) -> Option<u32> {
    // lsof를 사용하여 포트를 사용 중인 프로세스 찾기
    let output = Command::new("lsof")
        .args(["-t", "-i", &format!(":{}", port)])
        .output()
        .ok()?;

    if output.status.success() {
        let pids = String::from_utf8_lossy(&output.stdout);
        // 첫 번째 PID 반환
        pids.lines().next()?.trim().parse().ok()
    } else {
        None
    }
}

#[cfg(windows)]
pub fn find_process_using_port(port: u16) -> Option<u32> {
    // netstat를 사용하여 포트를 사용 중인 프로세스 찾기
    let output = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout);
        for line in lines.lines() {
            if line.contains(&format!(":{}", port)) && line.contains("LISTENING") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(pid_str) = parts.last() {
                    return pid_str.parse().ok();
                }
            }
        }
    }
    None
}

/// 프로세스 업타임 가져오기 (초)
#[cfg(unix)]
pub fn get_process_uptime(pid: u32) -> Option<u64> {
    // Linux: /proc/[pid]/stat에서 시작 시간 읽기
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        let stat_path = format!("/proc/{}/stat", pid);
        if let Ok(stat) = fs::read_to_string(&stat_path) {
            let fields: Vec<&str> = stat.split_whitespace().collect();
            if fields.len() > 21 {
                // 필드 22: starttime (클럭 틱)
                if let Ok(start_ticks) = fields[21].parse::<u64>() {
                    // 시스템 업타임에서 계산
                    if let Ok(uptime_str) = fs::read_to_string("/proc/uptime")
                        && let Some(uptime_secs_str) = uptime_str.split_whitespace().next()
                        && let Ok(system_uptime) = uptime_secs_str.parse::<f64>()
                    {
                        // 클럭 틱 -> 초 (보통 100 Hz)
                        let clock_ticks_per_sec = 100u64; // sysconf(_SC_CLK_TCK)
                        let start_time_secs = start_ticks / clock_ticks_per_sec;
                        let process_uptime = (system_uptime as u64).saturating_sub(start_time_secs);
                        return Some(process_uptime);
                    }
                }
            }
        }
        None
    }

    // macOS: ps 명령 사용
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-o", "etime=", "-p", &pid.to_string()])
            .output()
            .ok()?;

        if output.status.success() {
            let etime = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return parse_etime(&etime);
        }
        None
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(windows)]
pub fn get_process_uptime(_pid: u32) -> Option<u64> {
    // Windows에서는 구현 복잡, 일단 None
    None
}

/// ps etime 형식 파싱 ([[DD-]HH:]MM:SS)
#[cfg(target_os = "macos")]
fn parse_etime(etime: &str) -> Option<u64> {
    let parts: Vec<&str> = etime.split(':').collect();
    match parts.len() {
        2 => {
            // MM:SS
            let mins: u64 = parts[0].parse().ok()?;
            let secs: u64 = parts[1].parse().ok()?;
            Some(mins * 60 + secs)
        }
        3 => {
            // HH:MM:SS 또는 DD-HH:MM:SS
            let first = parts[0];
            if first.contains('-') {
                // DD-HH
                let day_hour: Vec<&str> = first.split('-').collect();
                let days: u64 = day_hour[0].parse().ok()?;
                let hours: u64 = day_hour[1].parse().ok()?;
                let mins: u64 = parts[1].parse().ok()?;
                let secs: u64 = parts[2].parse().ok()?;
                Some(days * 86400 + hours * 3600 + mins * 60 + secs)
            } else {
                // HH:MM:SS
                let hours: u64 = first.parse().ok()?;
                let mins: u64 = parts[1].parse().ok()?;
                let secs: u64 = parts[2].parse().ok()?;
                Some(hours * 3600 + mins * 60 + secs)
            }
        }
        _ => None,
    }
}

/// 시간 포맷팅
pub fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}
