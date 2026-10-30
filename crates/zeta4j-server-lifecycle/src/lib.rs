//! Shared server lifecycle primitives for the edition daemons and `z4j`.
//!
//! 서버 프로세스의 상태·PID·홈 디렉터리, readiness와 system 진단 유틸리티를 제공합니다.

pub mod health;
pub mod home;
pub mod process;
pub mod system;

pub use health::*;
pub use home::*;
pub use process::*;
pub use system::*;
