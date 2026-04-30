#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimePlatform {
    DarwinArm64,
    DarwinX64,
    WindowsX64,
    LinuxX64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePlatformError {
    UnsupportedPlatform { os: String, arch: String },
}

impl std::fmt::Display for RuntimePlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform { os, arch } => {
                write!(f, "unsupported runtime platform: {os}/{arch}")
            }
        }
    }
}

impl std::error::Error for RuntimePlatformError {}

impl RuntimePlatform {
    pub fn current() -> Result<Self, RuntimePlatformError> {
        Self::from_os_arch(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn from_os_arch(os: &str, arch: &str) -> Result<Self, RuntimePlatformError> {
        match (os, arch) {
            ("macos", "aarch64") => Ok(Self::DarwinArm64),
            ("macos", "x86_64") => Ok(Self::DarwinX64),
            ("windows", "x86_64") => Ok(Self::WindowsX64),
            ("linux", "x86_64") => Ok(Self::LinuxX64),
            _ => Err(RuntimePlatformError::UnsupportedPlatform {
                os: os.to_string(),
                arch: arch.to_string(),
            }),
        }
    }

    pub fn manifest_key(self) -> &'static str {
        match self {
            Self::DarwinArm64 => "darwin-arm64",
            Self::DarwinX64 => "darwin-x64",
            Self::WindowsX64 => "win32-x64",
            Self::LinuxX64 => "linux-x64",
        }
    }
}
