//! Gateway host — single source of truth for the Lotus tenant gateway origin.
//!
//! All three on-the-wire entrypoints (auth/billing in `auth::client`, the LLM
//! anthropic ingress in `llm::providers::lotus`, the v2 responses route in
//! `llm::providers::aijia_gateway_v2`) share one origin
//! `https://ai-tenant.renlijia.com`; only the path differs. So "switching
//! gateways" is switching this one host — callers build `format!("{host}{path}")`
//! against [`gateway_host()`].
//!
//! ## Dev-only override
//!
//! In **debug builds** the host can be overridden at runtime via a global
//! config key so developers can point at test/pre/local backends. In
//! **release builds** the entire override path (`#[cfg(debug_assertions)]`
//! below) is not compiled — [`gateway_host()`] unconditionally returns the
//! production constant. There is no runtime flag to flip; the isolation is
//! enforced by the compiler, so a tampered `config.json` has no effect on a
//! shipped binary.

/// Production gateway origin. No trailing slash.
pub const PROD_GATEWAY: &str = "https://ai-tenant.renlijia.com";

/// Returns the currently effective gateway origin (no trailing slash).
///
/// Release: always [`PROD_GATEWAY`]. Debug: the dev override if one is set and
/// non-empty, otherwise [`PROD_GATEWAY`].
pub fn gateway_host() -> String {
    #[cfg(debug_assertions)]
    {
        if let Some(host) = dev::current_override() {
            return host;
        }
    }
    PROD_GATEWAY.to_string()
}

/// Dev-only gateway override machinery. None of this is compiled into release
/// builds.
#[cfg(debug_assertions)]
pub mod dev {
    use std::sync::RwLock;

    use super::PROD_GATEWAY;

    /// Config key under `~/.renlijia/global/config.json` holding the override.
    pub const CONFIG_KEY: &str = "dev_gateway_host";

    /// Built-in presets shown in the dev switcher, ordered test → pre → prod.
    /// Test / pre environments are the production domain with a `test-` / `pre-`
    /// host prefix.
    pub const PRESETS: &[(&str, &str)] = &[
        ("测试", "https://test-ai-tenant.renlijia.com"),
        ("预发", "https://pre-ai-tenant.renlijia.com"),
        ("生产", PROD_GATEWAY),
    ];

    /// Process-level cache of the override. `gateway_host()` is called deep in
    /// auth/LLM paths that have no app handle, so the value is loaded once at
    /// startup ([`load`]) and updated by the dev command ([`set`]).
    static OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

    /// Normalize a user-entered host: trim whitespace and a trailing slash.
    /// Returns `None` for empty input (meaning "use production").
    fn normalize(raw: &str) -> Option<String> {
        let h = raw.trim().trim_end_matches('/');
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    }

    /// Current override, if any. `None` means production is in effect.
    pub fn current_override() -> Option<String> {
        OVERRIDE.read().ok().and_then(|g| g.clone())
    }

    /// The host the app is currently talking to (override or production).
    pub fn effective_host() -> String {
        current_override().unwrap_or_else(|| PROD_GATEWAY.to_string())
    }

    /// Seed the cache from persisted config at startup.
    pub fn load(persisted: Option<&str>) {
        let value = persisted.and_then(normalize);
        if let Ok(mut g) = OVERRIDE.write() {
            *g = value;
        }
    }

    /// Update the in-memory override. Returns the normalized value (`None` =
    /// production) so the caller can persist the same thing. Persistence is the
    /// caller's job (the command writes config).
    pub fn set(raw: &str) -> Option<String> {
        let value = normalize(raw);
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
        assert_eq!(gateway_host(), PROD_GATEWAY);
        assert_eq!(dev::current_override(), None);

        // Override takes effect and trailing slash is stripped.
        dev::set("https://test-ai-tenant.renlijia.com/");
        assert_eq!(gateway_host(), "https://test-ai-tenant.renlijia.com");

        // Loading a persisted value seeds the cache.
        dev::load(Some("https://pre-ai-tenant.renlijia.com"));
        assert_eq!(gateway_host(), "https://pre-ai-tenant.renlijia.com");

        // Empty / whitespace clears back to production.
        dev::set("   ");
        assert_eq!(gateway_host(), PROD_GATEWAY);
        assert_eq!(dev::current_override(), None);
    }
}
