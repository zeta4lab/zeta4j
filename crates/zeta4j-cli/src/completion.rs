//! Shell Completion 생성
//!
//! CLI 도구들의 shell completion 스크립트를 생성합니다.
//!
//! ## 지원 셸
//!
//! - Bash
//! - Zsh
//! - Fish
//! - PowerShell
//! - Elvish
//!
//! ## 사용법
//!
//! ```bash
//! # Bash
//! z4j completion bash > /etc/bash_completion.d/z4j
//!
//! # Zsh
//! z4j completion zsh > ~/.zfunc/_z4g
//!
//! # Fish
//! z4j completion fish > ~/.config/fish/completions/z4j.fish
//!
//! # PowerShell
//! z4j completion powershell >> $PROFILE
//! ```

use std::io::Write;

use clap::{Command, ValueEnum};
use clap_complete::{Shell, generate};

/// Supported shell types
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellType {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// PowerShell
    Powershell,
    /// Elvish shell
    Elvish,
}

impl From<ShellType> for Shell {
    fn from(shell: ShellType) -> Self {
        match shell {
            ShellType::Bash => Shell::Bash,
            ShellType::Zsh => Shell::Zsh,
            ShellType::Fish => Shell::Fish,
            ShellType::Powershell => Shell::PowerShell,
            ShellType::Elvish => Shell::Elvish,
        }
    }
}

/// Shell completion 스크립트 생성
///
/// # Arguments
/// * `cmd` - clap Command
/// * `shell` - 대상 셸 타입
/// * `bin_name` - 바이너리 이름
/// * `buf` - 출력 버퍼
pub fn generate_completion<W: Write>(
    cmd: &mut Command,
    shell: ShellType,
    bin_name: &str,
    buf: &mut W,
) {
    generate(Shell::from(shell), cmd, bin_name, buf);
}

/// Shell completion 스크립트를 문자열로 반환
pub fn generate_completion_string(cmd: &mut Command, shell: ShellType, bin_name: &str) -> String {
    let mut buf = Vec::new();
    generate_completion(cmd, shell, bin_name, &mut buf);
    String::from_utf8(buf).unwrap_or_default()
}

/// Per-shell installation instructions
pub fn installation_hint(shell: ShellType, bin_name: &str) -> String {
    match shell {
        ShellType::Bash => format!(
            r#"# Install Bash completion
# System-wide (requires root):
sudo {bin_name} completion bash > /etc/bash_completion.d/{bin_name}

# Per user:
mkdir -p ~/.local/share/bash-completion/completions
{bin_name} completion bash > ~/.local/share/bash-completion/completions/{bin_name}

# Enable in the current shell only:
source <({bin_name} completion bash)"#
        ),
        ShellType::Zsh => format!(
            r#"# Install Zsh completion
# 1. Make sure the completion directory is in fpath
# 2. Run:
mkdir -p ~/.zfunc
{bin_name} completion zsh > ~/.zfunc/_{bin_name}

# Add to ~/.zshrc:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit

# With Oh My Zsh:
{bin_name} completion zsh > $ZSH/completions/_{bin_name}"#
        ),
        ShellType::Fish => format!(
            r#"# Install Fish completion
mkdir -p ~/.config/fish/completions
{bin_name} completion fish > ~/.config/fish/completions/{bin_name}.fish"#
        ),
        ShellType::Powershell => format!(
            r#"# Install PowerShell completion
# Add to your PowerShell profile:
{bin_name} completion powershell >> $PROFILE

# Or use in the current session only:
{bin_name} completion powershell | Out-String | Invoke-Expression"#
        ),
        ShellType::Elvish => format!(
            r#"# Install Elvish completion
mkdir -p ~/.elvish/lib/completions
{bin_name} completion elvish > ~/.elvish/lib/completions/{bin_name}.elv

# Add to ~/.elvish/rc.elv:
use completions/{bin_name}"#
        ),
    }
}
