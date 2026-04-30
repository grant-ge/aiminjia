//! PowerShell discovery for Windows. Prefers `pwsh.exe` (7+ Core, supports
//! `&&`/`||` pipeline chain operators) over `powershell.exe` (5.1 Desktop,
//! parser error on `&&`). Memoized for the process lifetime.

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerShellEdition {
    /// pwsh.exe, PowerShell 7+. Supports `&&`, `||`, `?:`, `??`.
    Core,
    /// powershell.exe, Windows PowerShell 5.1. No pipeline chain operators.
    Desktop,
}

#[derive(Debug, Clone)]
pub struct PowerShellLocation {
    pub path: PathBuf,
    pub edition: PowerShellEdition,
}

static CACHED: OnceLock<Option<PowerShellLocation>> = OnceLock::new();

pub fn detect() -> Option<PowerShellLocation> {
    CACHED.get_or_init(detect_uncached).clone()
}

fn detect_uncached() -> Option<PowerShellLocation> {
    if let Ok(p) = which::which("pwsh") {
        return Some(PowerShellLocation {
            path: p,
            edition: PowerShellEdition::Core,
        });
    }
    if let Ok(p) = which::which("powershell") {
        return Some(PowerShellLocation {
            path: p,
            edition: PowerShellEdition::Desktop,
        });
    }
    // Last-ditch: hard-coded Windows 5.1 install path. powershell.exe ships
    // with every Windows 7+ install, so PATH-resolution failure is unusual
    // but not impossible (corporate-locked PATH, broken user profile).
    let fallback = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    if fallback.exists() {
        return Some(PowerShellLocation {
            path: fallback,
            edition: PowerShellEdition::Desktop,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_some_on_windows_dev_machine() {
        // 在真实 Windows 上至少能找到 powershell.exe（5.1 自带）。
        let result = detect_uncached();
        assert!(
            result.is_some(),
            "Windows must always have at least powershell.exe at the fallback path"
        );
    }

    #[test]
    fn pwsh_preferred_over_powershell_when_both_present() {
        // 文档化优先级：找到 pwsh 时一定返回 Core；否则返回 Desktop。
        let result = detect_uncached().expect("Windows must have powershell available");
        let basename = result
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        match result.edition {
            PowerShellEdition::Core => assert!(
                basename.starts_with("pwsh"),
                "Core edition must come from pwsh executable: {basename}"
            ),
            PowerShellEdition::Desktop => assert!(
                basename.starts_with("powershell"),
                "Desktop edition must come from powershell executable: {basename}"
            ),
        }
    }
}
