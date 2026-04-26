use app_lib::runtime::dependencies::{RuntimeManifest, RuntimeManifestError, RuntimePlatform};

fn valid_manifest_json() -> &'static str {
    r#"
    {
      "bundleVersion": "2026.04.25-test",
      "source": "unit-test",
      "runtimes": {
        "node": {
          "version": "22.15.0",
          "platforms": {
            "darwin-arm64": {
              "url": "https://example.invalid/node-darwin-arm64.tar.gz",
              "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            }
          }
        }
      }
    }
    "#
}

#[test]
fn selects_node_darwin_arm64_artifact() {
    let manifest = RuntimeManifest::from_json(valid_manifest_json()).expect("valid manifest");

    let artifact = manifest
        .artifact("node", RuntimePlatform::DarwinArm64)
        .expect("node darwin-arm64 artifact");

    assert_eq!(
        artifact.url,
        "https://example.invalid/node-darwin-arm64.tar.gz"
    );
    assert_eq!(
        artifact.sha256,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
}

#[test]
fn missing_runtime_or_platform_returns_missing_runtime() {
    let manifest = RuntimeManifest::from_json(valid_manifest_json()).expect("valid manifest");

    assert_eq!(
        manifest.artifact("python", RuntimePlatform::DarwinArm64),
        Err(RuntimeManifestError::MissingRuntime {
            name: "python".to_string(),
            platform: "darwin-arm64".to_string(),
        })
    );
    assert_eq!(
        manifest.artifact("node", RuntimePlatform::LinuxX64),
        Err(RuntimeManifestError::MissingRuntime {
            name: "node".to_string(),
            platform: "linux-x64".to_string(),
        })
    );
}

#[test]
fn invalid_sha256_returns_invalid_sha256() {
    let json = r#"
    {
      "bundleVersion": "2026.04.25-test",
      "source": "unit-test",
      "runtimes": {
        "node": {
          "version": "22.15.0",
          "platforms": {
            "darwin-arm64": {
              "url": "https://example.invalid/node-darwin-arm64.tar.gz",
              "sha256": "not-a-valid-sha256"
            }
          }
        }
      }
    }
    "#;

    assert_eq!(
        RuntimeManifest::from_json(json),
        Err(RuntimeManifestError::InvalidSha256 {
            name: "node".to_string(),
            sha256: "not-a-valid-sha256".to_string(),
        })
    );
}

#[test]
fn parses_runtime_manifest_test_fixture() {
    let manifest = RuntimeManifest::from_json(include_str!("../runtime-manifest.test.json"))
        .expect("fixture manifest should parse");

    assert_eq!(manifest.source, "test-fixture");
    assert!(manifest
        .artifact("node", RuntimePlatform::DarwinArm64)
        .is_ok());
    assert!(manifest
        .artifact("python", RuntimePlatform::DarwinArm64)
        .is_ok());
    assert!(manifest
        .artifact("uv", RuntimePlatform::DarwinArm64)
        .is_ok());
}

#[test]
fn rejects_empty_manifest_or_empty_platforms() {
    let empty_manifest = r#"
    {
      "bundleVersion": "2026.04.25-test",
      "source": "unit-test",
      "runtimes": {}
    }
    "#;

    assert_eq!(
        RuntimeManifest::from_json(empty_manifest),
        Err(RuntimeManifestError::EmptyRuntimes)
    );

    let empty_platforms = r#"
    {
      "bundleVersion": "2026.04.25-test",
      "source": "unit-test",
      "runtimes": {
        "node": {
          "version": "22.15.0",
          "platforms": {}
        }
      }
    }
    "#;

    assert_eq!(
        RuntimeManifest::from_json(empty_platforms),
        Err(RuntimeManifestError::EmptyPlatforms {
            name: "node".to_string(),
        })
    );
}

#[test]
fn rejects_untrusted_artifact_urls() {
    for url in [
        "http://example.invalid/node.tar.gz",
        "https://localhost/node.tar.gz",
        "https://127.0.0.1/node.tar.gz",
        "../node.tar.gz",
    ] {
        let json = format!(
            r#"{{
              "bundleVersion": "2026.04.25-test",
              "source": "unit-test",
              "runtimes": {{
                "node": {{
                  "version": "22.15.0",
                  "platforms": {{
                    "darwin-arm64": {{
                      "url": "{url}",
                      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    }}
                  }}
                }}
              }}
            }}"#
        );

        assert_eq!(
            RuntimeManifest::from_json(&json),
            Err(RuntimeManifestError::UntrustedArtifactUrl {
                name: "node".to_string(),
                platform: "darwin-arm64".to_string(),
                url: url.to_string(),
            })
        );
    }
}


#[test]
fn rejects_file_artifact_url_for_production_manifest_source() {
    let json = r#"{
      "bundleVersion": "2026.04.25-test",
      "source": "production",
      "runtimes": {
        "node": {
          "version": "22.15.0",
          "platforms": {
            "darwin-arm64": {
              "url": "file:///tmp/node.tar.gz",
              "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            }
          }
        }
      }
    }"#;

    assert_eq!(
        RuntimeManifest::from_json(json),
        Err(RuntimeManifestError::UntrustedArtifactUrl {
            name: "node".to_string(),
            platform: "darwin-arm64".to_string(),
            url: "file:///tmp/node.tar.gz".to_string(),
        })
    );
}

#[test]
fn parses_production_manifest_fields_for_channel_provider_size_and_rollback() {
    let json = r#"
    {
      "bundleVersion": "2026.05.20",
      "channel": "stable",
      "minimumAppVersion": "0.4.16",
      "defaultProvider": "renlijia-bundle",
      "source": "unit-test",
      "rollback": {
        "bundleVersion": "2026.05.19",
        "reason": "known good fallback"
      },
      "mirrors": ["https://mirror.example.com/runtimes/"],
      "runtimes": {
        "primary": {
          "version": "2026.05.20",
          "platforms": {
            "darwin-arm64": {
              "url": "https://download.example.com/runtime.zip",
              "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
              "sizeBytes": 123456,
              "archiveFormat": "zip"
            }
          }
        }
      }
    }
    "#;

    let manifest = RuntimeManifest::from_json(json).expect("production manifest should parse");
    let artifact = manifest.artifact("primary", RuntimePlatform::DarwinArm64).unwrap();

    assert_eq!(manifest.channel.as_deref(), Some("stable"));
    assert_eq!(manifest.minimum_app_version.as_deref(), Some("0.4.16"));
    assert_eq!(manifest.default_provider.as_deref(), Some("renlijia-bundle"));
    assert_eq!(manifest.rollback.as_ref().unwrap().bundle_version, "2026.05.19");
    assert_eq!(manifest.mirrors, vec!["https://mirror.example.com/runtimes/"]);
    assert_eq!(artifact.size_bytes, Some(123456));
    assert_eq!(artifact.archive_format.as_deref(), Some("zip"));
}

#[test]
fn rejects_invalid_manifest_size_and_archive_format() {
    let invalid_size = r#"{
      "bundleVersion": "2026.05.20",
      "source": "unit-test",
      "runtimes": {"primary": {"version": "2026.05.20", "platforms": {"darwin-arm64": {
        "url": "https://download.example.com/runtime.zip",
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sizeBytes": 0,
        "archiveFormat": "zip"
      }}}}
    }"#;
    assert!(RuntimeManifest::from_json(invalid_size).is_err());

    let invalid_format = invalid_size.replace("\"zip\"", "\"rar\"").replace("\"sizeBytes\": 0", "\"sizeBytes\": 1");
    assert!(RuntimeManifest::from_json(&invalid_format).is_err());
}
