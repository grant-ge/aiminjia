use app_lib::runtime::dependencies::{RuntimePlatform, RuntimePlatformError};

#[test]
fn maps_supported_os_arch_pairs_to_manifest_keys() {
    let cases = [
        (
            "macos",
            "aarch64",
            RuntimePlatform::DarwinArm64,
            "darwin-arm64",
        ),
        ("macos", "x86_64", RuntimePlatform::DarwinX64, "darwin-x64"),
        (
            "windows",
            "x86_64",
            RuntimePlatform::WindowsX64,
            "win32-x64",
        ),
        ("linux", "x86_64", RuntimePlatform::LinuxX64, "linux-x64"),
    ];

    for (os, arch, expected_platform, expected_key) in cases {
        let platform = RuntimePlatform::from_os_arch(os, arch).expect("supported platform");
        assert_eq!(platform, expected_platform);
        assert_eq!(platform.manifest_key(), expected_key);
    }
}

#[test]
fn rejects_unknown_os_arch_pair_with_original_values() {
    let error = RuntimePlatform::from_os_arch("freebsd", "riscv64").unwrap_err();

    assert_eq!(
        error,
        RuntimePlatformError::UnsupportedPlatform {
            os: "freebsd".to_string(),
            arch: "riscv64".to_string(),
        }
    );
}

#[test]
fn current_platform_is_supported_or_returns_unsupported_error() {
    match RuntimePlatform::current() {
        Ok(platform) => assert!(!platform.manifest_key().is_empty()),
        Err(RuntimePlatformError::UnsupportedPlatform { os, arch }) => {
            assert_eq!(os, std::env::consts::OS);
            assert_eq!(arch, std::env::consts::ARCH);
        }
    }
}
