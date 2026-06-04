use std::path::{Path, PathBuf};

use super::{RuntimePlatform, RuntimePlatformError, WorkspaceDependencies};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLayout {
    platform: RuntimePlatform,
}

impl RuntimeLayout {
    pub fn current() -> Result<Self, RuntimePlatformError> {
        RuntimePlatform::current().map(Self::for_platform)
    }

    pub fn for_platform(platform: RuntimePlatform) -> Self {
        Self { platform }
    }

    pub fn platform(self) -> RuntimePlatform {
        self.platform
    }

    pub fn python(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "python/python.exe",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "python/bin/python3",
        }
    }

    pub fn node(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "node/node.exe",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "node/bin/node",
        }
    }

    pub fn npm(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "node/npm.cmd",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "node/bin/npm",
        }
    }

    pub fn npx(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "node/npx.cmd",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "node/bin/npx",
        }
    }

    pub fn uv(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "uv/uv.exe",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "uv/bin/uv",
        }
    }

    pub fn uvx(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "uv/uvx.exe",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "uv/bin/uvx",
        }
    }

    pub fn node_modules(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "node/node_modules",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "node/lib/node_modules",
        }
    }

    pub fn python_site_packages(self) -> &'static str {
        match self.platform {
            RuntimePlatform::WindowsX64 => "python/Lib/site-packages",
            RuntimePlatform::DarwinArm64
            | RuntimePlatform::DarwinX64
            | RuntimePlatform::LinuxX64 => "python/lib/python3.12/site-packages",
        }
    }

    pub fn executable_paths(self) -> [&'static str; 6] {
        [
            self.python(),
            self.node(),
            self.npm(),
            self.npx(),
            self.uv(),
            self.uvx(),
        ]
    }

    pub fn directory_paths(self) -> [&'static str; 2] {
        [self.node_modules(), self.python_site_packages()]
    }

    pub fn workspace_dependencies(self, install_dir: &Path) -> WorkspaceDependencies {
        WorkspaceDependencies {
            python: install_dir.join(self.python()),
            node: install_dir.join(self.node()),
            npm: install_dir.join(self.npm()),
            npx: install_dir.join(self.npx()),
            uv: install_dir.join(self.uv()),
            uvx: install_dir.join(self.uvx()),
            node_modules: install_dir.join(self.node_modules()),
            python_site_packages: install_dir.join(self.python_site_packages()),
        }
    }

    pub fn install_dir_from_python_path(self, python_path: &Path) -> Option<PathBuf> {
        let relative = Path::new(self.python());
        let depth = relative.components().count();
        let mut current = python_path;
        for _ in 0..depth {
            current = current.parent()?;
        }
        Some(current.to_path_buf())
    }
}
