//! logs 명령 구현

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use crate::ctl::utils::get_log_path;

/// 로그 보기
pub fn cmd_logs(home: PathBuf, follow: bool, lines: usize) -> ExitCode {
    let log_path = get_log_path(&home);

    if !log_path.exists() {
        eprintln!("Log file not found: {}", log_path.display());
        return ExitCode::FAILURE;
    }

    if follow {
        // tail -f 모드
        let mut file = match File::open(&log_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Failed to open log file: {}", e);
                return ExitCode::FAILURE;
            }
        };

        // 파일 끝으로 이동
        let _ = file.seek(SeekFrom::End(0));

        // 마지막 N줄 출력
        print_last_lines(&log_path, lines);

        println!("--- Following log (Ctrl+C to stop) ---");

        // 새 내용 계속 읽기
        let mut buffer = String::new();
        loop {
            match file.read_to_string(&mut buffer) {
                Ok(0) => {
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(_) => {
                    print!("{}", buffer);
                    if let Err(error) = std::io::stdout().flush() {
                        eprintln!("Error writing log output: {error}");
                        return ExitCode::FAILURE;
                    }
                    buffer.clear();
                }
                Err(e) => {
                    eprintln!("Error reading log: {}", e);
                    break;
                }
            }
        }
    } else {
        print_last_lines(&log_path, lines);
    }

    ExitCode::SUCCESS
}

/// 마지막 N줄 출력
pub fn print_last_lines(path: &PathBuf, n: usize) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };

    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    let start = if lines.len() > n { lines.len() - n } else { 0 };
    for line in &lines[start..] {
        println!("{}", line);
    }
}
