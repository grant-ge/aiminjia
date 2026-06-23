//! Environment — the backend origins the desktop app talks to.
//!
//! An environment bundles the two ingress origins the app needs:
//! - `tenant`: the Lotus tenant gateway — auth/billing, the LLM ingress, and
//!   the `/v1/*` desktop APIs (search, diagnostics, skill packages, …).
//! - `ops`: the Lotus ops-portal — the public employee-template catalog.
//!
//! Both move together when switching environments (test → pre → prod), so they
//! live on one [`Environment`] object rather than being scattered as per-call
//! constants. Callers read [`tenant_host()`] / [`ops_host()`].
//!
//! ## Dev-only override
//!
//! In **debug builds** the environment can be overridden at runtime so
//! developers can point at test/pre/custom backends. In **release builds** the
//! entire override path (`#[cfg(debug_assertions)]` below) is not compiled —
//! [`current()`] unconditionally returns [production](prod). There is no
//! runtime flag to flip; the isolation is enforced by the compiler, so a
//! tampered `config.json` has no effect on a shipped binary.

/// Production tenant gateway origin. No trailing slash.
pub const PROD_TENANT: &str = "https://ai-tenant.renlijia.com";
/// Production ops-portal origin. No trailing slash.
pub const PROD_OPS: &str = "https://ai-ops.renlijia.com";

/// A resolved environment: the two origins the app talks to (no trailing slash).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub tenant: String,
    pub ops: String,
}

impl Environment {
    /// The production environment.
    pub fn prod() -> Self {
        Self {
            tenant: PROD_TENANT.to_string(),
            ops: PROD_OPS.to_string(),
        }
    }
}

/// Returns the currently effective environment.
///
/// Release: always [production](Environment::prod). Debug: the dev override if
/// one is set, otherwise production.
pub fn current() -> Environment {
    #[cfg(debug_assertions)]
    {
        if let Some(env) = dev::current_override() {
            return env;
        }
    }
    Environment::prod()
}

/// The tenant gateway origin currently in effect (no trailing slash).
pub fn tenant_host() -> String {
    current().tenant
}

/// The ops-portal origin currently in effect (no trailing slash).
pub fn ops_host() -> String {
    current().ops
}

/// Dev-only environment override machinery. None of this is compiled into
/// release builds.
#[cfg(debug_assertions)]
pub mod dev {
    use std::sync::RwLock;

    use super::{Environment, PROD_OPS, PROD_TENANT};

    /// Config key under `~/.renlijia/global/config.json` holding the override,
    /// serialized as `{"tenant": "...", "ops": "..."}`.
    pub const CONFIG_KEY: &str = "dev_environment";

    /// Built-in presets shown in the dev switcher, ordered test → pre → prod.
    /// Each is `(key, tenant, ops)`. `key` is a stable, language-neutral id the
    /// frontend translates for display (it must NOT be a localized string).
    /// Test / pre share the production domain with a `test-` / `pre-` host
    /// prefix on both origins.
    pub const PRESETS: &[(&str, &str, &str)] = &[
        (
            "test",
            "https://test-ai-tenant.renlijia.com",
            "https://test-ai-ops.renlijia.com",
        ),
        (
            "pre",
            "https://pre-ai-tenant.renlijia.com",
            "https://pre-ai-ops.renlijia.com",
        ),
        ("prod", PROD_TENANT, PROD_OPS),
    ];

    /// Process-level cache of the override. `current()` is called deep in
    /// auth/LLM/ops paths that have no app handle, so the value is loaded once
    /// at startup ([`load`]) and updated by the dev command ([`set`]).
    static OVERRIDE: RwLock<Option<Environment>> = RwLock::new(None);

    /// Normalize a single user-entered origin: trim whitespace and a trailing
    /// slash. Returns `None` for empty input.
    fn normalize_origin(raw: &str) -> Option<String> {
        let h = raw.trim().trim_end_matches('/');
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    }

    /// Build an override from a `(tenant, ops)` pair. Returns `None` (meaning
    /// "use production") when either origin is empty or the pair equals
    /// production — so picking the prod preset clears the override.
    pub fn normalize(tenant: &str, ops: &str) -> Option<Environment> {
        match (normalize_origin(tenant), normalize_origin(ops)) {
            (Some(t), Some(o)) => {
                if t == PROD_TENANT && o == PROD_OPS {
                    None
                } else {
                    Some(Environment { tenant: t, ops: o })
                }
            }
            _ => None,
        }
    }

    /// Current override, if any. `None` means production is in effect.
    pub fn current_override() -> Option<Environment> {
        OVERRIDE.read().ok().and_then(|g| g.clone())
    }

    /// The environment the app is currently talking to (override or production).
    pub fn effective() -> Environment {
        current_override().unwrap_or_else(Environment::prod)
    }

    /// Serialize an environment to the persisted JSON shape.
    pub fn serialize(env: &Environment) -> String {
        serde_json::json!({ "tenant": env.tenant, "ops": env.ops }).to_string()
    }

    /// Parse a persisted JSON override. Returns `None` on any malformed input
    /// (treated as "no override").
    fn parse(raw: &str) -> Option<Environment> {
        let v: serde_json::Value = serde_json::from_str(raw).ok()?;
        let tenant = v.get("tenant").and_then(|x| x.as_str())?;
        let ops = v.get("ops").and_then(|x| x.as_str())?;
        normalize(tenant, ops)
    }

    /// Seed the cache from persisted config at startup.
    pub fn load(persisted: Option<&str>) {
        let value = persisted.and_then(parse);
        if let Ok(mut g) = OVERRIDE.write() {
            *g = value;
        }
    }

    /// Update the in-memory override. Returns the normalized value (`None` =
    /// production) so the caller can persist the same thing. Persistence is the
    /// caller's job (the command writes config).
    pub fn set(tenant: &str, ops: &str) -> Option<Environment> {
        let value = normalize(tenant, ops);
        if let Ok(mut g) = OVERRIDE.write() {
            *g = value.clone();
        }
        value
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    // Single test: the override lives in a process-global static, so splitting
    // these into parallel tests would race on shared state.
    #[test]
    fn dev_override_lifecycle() {
        // Default: production.
        dev::load(None);
        assert_eq!(current(), Environment::prod());
        assert_eq!(tenant_host(), PROD_TENANT);
        assert_eq!(ops_host(), PROD_OPS);
        assert_eq!(dev::current_override(), None);

        // Override takes effect and trailing slashes are stripped on both.
        dev::set(
            "https://test-ai-tenant.renlijia.com/",
            "https://test-ai-ops.renlijia.com/",
        );
        assert_eq!(tenant_host(), "https://test-ai-tenant.renlijia.com");
        assert_eq!(ops_host(), "https://test-ai-ops.renlijia.com");

        // Loading a persisted JSON value seeds the cache.
        dev::load(Some(
            r#"{"tenant":"https://pre-ai-tenant.renlijia.com","ops":"https://pre-ai-ops.renlijia.com"}"#,
        ));
        assert_eq!(tenant_host(), "https://pre-ai-tenant.renlijia.com");
        assert_eq!(ops_host(), "https://pre-ai-ops.renlijia.com");

        // Round-trip through serialize/parse.
        let env = dev::current_override().unwrap();
        dev::load(Some(&dev::serialize(&env)));
        assert_eq!(dev::current_override(), Some(env));

        // Picking the production pair clears the override.
        dev::set(PROD_TENANT, PROD_OPS);
        assert_eq!(dev::current_override(), None);

        // Empty input clears back to production.
        dev::set("https://x.example.com", "");
        assert_eq!(dev::current_override(), None);
    }
}
