#![cfg_attr(
    test,
    allow(
        unused_imports,
        dead_code,
        unused_variables,
        clippy::module_inception,
        clippy::needless_borrows_for_generic_args,
        clippy::assertions_on_constants,
        clippy::bool_assert_comparison,
        clippy::unnecessary_cast
    )
)]

pub mod auth;
pub mod cache;
pub mod email;
pub mod encryption;
pub mod hmac;
pub mod models;
pub mod password_policy;
pub mod password_reset;
pub mod redis;
pub mod rls;
pub mod services;
pub mod ssm;

#[cfg(test)]
mod encryption_compatibility_tests;

#[cfg(feature = "otp")]
pub mod otp;

#[cfg(feature = "webauthn")]
pub mod webauthn;

/// Normalize environment name for SSM paths
/// Converts "development" -> "dev", "production" -> "prod"
pub fn normalize_env_for_ssm(env: &str) -> &str {
    match env {
        "development" => "dev",
        "production" => "prod",
        other => other,
    }
}

/// Normalize environment name for Redis key prefixes
/// Uses the same normalization as SSM
pub fn normalize_env_for_redis(env: &str) -> &str {
    normalize_env_for_ssm(env)
}
