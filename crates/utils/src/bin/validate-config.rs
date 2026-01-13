use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use leadsnebula_core::normalize_env_for_ssm;
use leadsnebula_core::ssm::SsmService;
use serde_json::json;
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "validate-config")]
#[command(about = "Validate configuration files and secrets for LeadsNebula")]
#[command(
    long_about = "A comprehensive validation tool for LeadsNebula Rust project configuration.

This tool validates:
- Fly.io configuration files (fly.toml, fly.dev.toml)
- Cargo.toml and Cargo.lock files
- Dockerfile structure and Rust version
- SSM parameter path formats
- Required secrets in .env.local
- GitHub Actions workflow YAML
- Duplicate dependencies in Cargo.lock

Examples:
  # Validate fly.toml
  validate-config fly-toml fly.dev.toml

  # Check secrets in .env.local
  validate-config secrets --check-local

  # Check SSM parameters (requires AWS credentials)
  validate-config secrets --check-local --ssm-check --env dev

  # Validate GitHub Actions workflow
  validate-config github-workflow .github/workflows/rust-ci.yml

  # Detect duplicate dependencies
  validate-config cargo-duplicates

  # Use strict mode (warnings become errors)
  validate-config secrets --check-local --strict

  # Verbose output with full details
  validate-config fly-toml fly.dev.toml --verbose"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output verbose JSON with full details (default: summary)
    #[arg(long, global = true)]
    verbose: bool,

    /// Treat warnings as errors (exit code 1 on warnings)
    #[arg(long, global = true)]
    strict: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse fly.toml and extract app name, primary_region
    ///
    /// Example:
    ///   validate-config fly-toml fly.dev.toml
    FlyToml {
        /// Path to fly.toml file
        path: PathBuf,
    },
    /// Parse Cargo.toml and extract version info
    CargoToml {
        /// Path to Cargo.toml file
        path: PathBuf,
    },
    /// Parse Dockerfile and extract Rust version, required binaries
    Dockerfile {
        /// Path to Dockerfile
        path: PathBuf,
    },
    /// Validate SSM parameter path formats
    SsmPaths {
        /// Environment name (default: from ENVIRONMENT or SSM_ENV env var, or "dev")
        #[arg(long)]
        env: Option<String>,
    },
    /// Validate GitHub Actions workflow YAML
    ///
    /// Checks for:
    /// - Valid YAML syntax
    /// - Required permissions (packages: write if pushing to GHCR)
    /// - Valid cargo llvm-cov command options
    ///
    /// Example:
    ///   validate-config github-workflow .github/workflows/rust-ci.yml
    GithubWorkflow {
        /// Path to workflow YAML file
        path: PathBuf,
    },
    /// Detect duplicate dependencies in Cargo.lock
    ///
    /// Runs `cargo tree --duplicates` and provides actionable warnings
    /// for packages with multiple versions.
    ///
    /// Example:
    ///   validate-config cargo-duplicates
    CargoDuplicates,
    /// Check for required secrets
    ///
    /// Validates that required secrets (DATABASE_URL, JWT_SECRET, ENCRYPTION_KEY, REDIS_URL)
    /// are present in .env.local and optionally checks SSM Parameter Store.
    ///
    /// Examples:
    ///   # Check .env.local only
    ///   validate-config secrets --check-local
    ///
    ///   # Check both .env.local and SSM (requires AWS credentials)
    ///   validate-config secrets --check-local --ssm-check --env dev
    Secrets {
        /// Environment name (default: from ENVIRONMENT or SSM_ENV env var, or "dev")
        #[arg(long)]
        env: Option<String>,

        /// Check .env.local file for secrets
        #[arg(long)]
        check_local: bool,

        /// Check SSM Parameter Store for parameter existence (requires AWS credentials)
        ///
        /// This will verify that SSM parameters exist without decrypting them.
        /// Requires AWS credentials (AWS_PROFILE or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY).
        #[arg(long)]
        ssm_check: bool,
    },
}

#[derive(Clone)]
struct ErrorMessage {
    message: String,
    remediation: Option<String>,
}

impl ErrorMessage {
    fn new(message: String) -> Self {
        Self {
            message,
            remediation: None,
        }
    }

    fn with_remediation(message: String, remediation: String) -> Self {
        Self {
            message,
            remediation: Some(remediation),
        }
    }
}

impl std::fmt::Display for ErrorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref remediation) = self.remediation {
            write!(f, " → {}", remediation)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ValidationResult {
    valid: bool,
    app_name: Option<String>,
    primary_region: Option<String>,
    rust_version: Option<String>,
    required_binaries: Vec<String>,
    warnings: Vec<ErrorMessage>,
    errors: Vec<ErrorMessage>,
    ssm_paths: HashMap<String, bool>,
    missing_secrets: Vec<String>,
    missing_ssm_params: Vec<String>,
}

impl ValidationResult {
    fn to_json(&self, verbose: bool) -> serde_json::Value {
        let mut result = json!({
            "valid": self.valid,
        });

        // Always include app_name and primary_region (needed for bash parsing)
        if let Some(app_name) = &self.app_name {
            result["app_name"] = json!(app_name);
        }
        if let Some(primary_region) = &self.primary_region {
            result["primary_region"] = json!(primary_region);
        }
        if let Some(rust_version) = &self.rust_version {
            result["rust_version"] = json!(rust_version);
        }
        if !self.required_binaries.is_empty() {
            result["required_binaries"] = json!(self.required_binaries);
        }

        if verbose && !self.ssm_paths.is_empty() {
            result["ssm_paths"] = json!(self.ssm_paths);
        }
        if !self.missing_ssm_params.is_empty() {
            result["missing_ssm_params"] = json!(self.missing_ssm_params);
        }

        // Convert ErrorMessage to JSON (with remediation if present)
        if !self.warnings.is_empty() {
            let warnings_json: Vec<serde_json::Value> = self
                .warnings
                .iter()
                .map(|w| {
                    if let Some(ref remediation) = w.remediation {
                        json!({
                            "message": w.message,
                            "remediation": remediation
                        })
                    } else {
                        json!(w.message)
                    }
                })
                .collect();
            result["warnings"] = json!(warnings_json);
        }
        if !self.errors.is_empty() {
            let errors_json: Vec<serde_json::Value> = self
                .errors
                .iter()
                .map(|e| {
                    if let Some(ref remediation) = e.remediation {
                        json!({
                            "message": e.message,
                            "remediation": remediation
                        })
                    } else {
                        json!(e.message)
                    }
                })
                .collect();
            result["errors"] = json!(errors_json);
        }
        if !self.missing_secrets.is_empty() {
            result["missing_secrets"] = json!(self.missing_secrets);
        }

        result
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut result = match &cli.command {
        Commands::FlyToml { path } => validate_fly_toml(path)?,
        Commands::CargoToml { path } => validate_cargo_toml(path)?,
        Commands::Dockerfile { path } => validate_dockerfile(path)?,
        Commands::SsmPaths { env } => validate_ssm_paths(env.as_deref())?,
        Commands::GithubWorkflow { path } => validate_github_workflow(path)?,
        Commands::CargoDuplicates => validate_cargo_duplicates()?,
        Commands::Secrets {
            env,
            check_local,
            ssm_check,
        } => validate_secrets(env.as_deref(), *check_local, *ssm_check).await?,
    };

    // In strict mode, treat warnings as errors
    if cli.strict && !result.warnings.is_empty() {
        result.errors.append(&mut result.warnings);
        result.valid = false;
    }

    // Output JSON
    let output = result.to_json(cli.verbose);
    println!("{}", serde_json::to_string_pretty(&output)?);

    // Exit with error code if invalid
    if !result.valid {
        std::process::exit(1);
    }

    Ok(())
}

fn validate_fly_toml(path: &PathBuf) -> Result<ValidationResult> {
    let mut result = ValidationResult::default();

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read fly.toml: {}", path.display()))?;

    let parsed: toml::Value =
        toml::from_str(&content).context("Failed to parse fly.toml as TOML")?;

    // Extract app name
    if let Some(app) = parsed.get("app").and_then(|v| v.as_str()) {
        result.app_name = Some(app.to_string());
        result.valid = true;
    } else {
        result.errors.push(ErrorMessage::with_remediation(
            "Missing 'app' field in fly.toml".to_string(),
            "Add 'app = \"your-app-name\"' to fly.toml".to_string(),
        ));
        result.valid = false;
    }

    // Extract primary_region (optional)
    if let Some(region) = parsed.get("primary_region").and_then(|v| v.as_str()) {
        result.primary_region = Some(region.to_string());
    }

    Ok(result)
}

fn validate_cargo_toml(path: &PathBuf) -> Result<ValidationResult> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Cargo.toml: {}", path.display()))?;

    let _parsed: toml::Value =
        toml::from_str(&content).context("Failed to parse Cargo.toml as TOML")?;

    // For now, just validate it's valid TOML
    // Phase 2 will add more specific checks
    let result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    Ok(result)
}

fn validate_dockerfile(path: &PathBuf) -> Result<ValidationResult> {
    let mut result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Dockerfile: {}", path.display()))?;

    // Parse Dockerfile line by line
    for line in content.lines() {
        let line = line.trim();

        // Extract Rust version from FROM rust:... lines
        if line.starts_with("FROM rust:") {
            let version_part = line
                .strip_prefix("FROM rust:")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("");
            if !version_part.is_empty() {
                result.rust_version = Some(version_part.to_string());
            }
        }

        // Extract required binaries from COPY commands
        if line.starts_with("COPY") && line.contains("target/release/") {
            // Extract binary names from COPY target/release/binary-name /path/to/dest
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in parts {
                if part.contains("target/release/") {
                    if let Some(binary) = part.strip_prefix("target/release/") {
                        if !binary.is_empty() && !binary.contains('/') {
                            result.required_binaries.push(binary.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

fn validate_ssm_paths(env: Option<&str>) -> Result<ValidationResult> {
    let mut result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    // Get environment
    let env = if let Some(e) = env {
        e.to_string()
    } else if let Ok(e) = std::env::var("ENVIRONMENT") {
        e
    } else if let Ok(e) = std::env::var("SSM_ENV") {
        e
    } else {
        "dev".to_string()
    };

    let env_normalized = normalize_env_for_ssm(&env);

    // Expected SSM paths
    let expected_paths = vec![
        (
            "DATABASE_URL",
            format!("/leadsnebula/{}/rust/db/connection_url", env_normalized),
        ),
        (
            "REDIS_URL",
            format!("/leadsnebula/{}/rust/redis/connection_url", env_normalized),
        ),
        (
            "JWT_SECRET",
            format!("/leadsnebula/{}/rust/auth/jwt_secret", env_normalized),
        ),
        (
            "ENCRYPTION_KEY",
            format!("/leadsnebula/{}/rust/encryption/primary_v1", env_normalized),
        ),
    ];

    // Validate path formats
    for (name, path) in expected_paths {
        if path.starts_with("/leadsnebula/") && path.contains(&format!("/{}/", env_normalized)) {
            result.ssm_paths.insert(name.to_string(), true);
        } else {
            result.warnings.push(ErrorMessage::new(format!(
                "Invalid SSM path format for {}: {}",
                name, path
            )));
            result.ssm_paths.insert(name.to_string(), false);
        }
    }

    Ok(result)
}

async fn validate_secrets(
    env: Option<&str>,
    check_local: bool,
    ssm_check: bool,
) -> Result<ValidationResult> {
    let mut result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    if !check_local {
        // Just validate SSM paths
        return validate_ssm_paths(env);
    }

    // Check .env.local file
    let env_local = PathBuf::from(".env.local");
    if !env_local.exists() {
        result.warnings.push(ErrorMessage::with_remediation(
            ".env.local not found - secrets cannot be validated locally".to_string(),
            "Create .env.local file or run: fly secrets list".to_string(),
        ));
        if !ssm_check {
            return Ok(result);
        }
    }

    let content = fs::read_to_string(&env_local).context("Failed to read .env.local")?;

    // Required secrets
    let required_secrets = vec!["DATABASE_URL", "JWT_SECRET", "ENCRYPTION_KEY", "REDIS_URL"];

    for secret in required_secrets {
        let found = content
            .lines()
            .any(|line| line.starts_with(&format!("{}=", secret)) && !line.trim().starts_with('#'));

        if !found {
            result.missing_secrets.push(secret.to_string());
            result.warnings.push(ErrorMessage::with_remediation(
                format!("Missing {} in .env.local", secret),
                format!("Set {} in .env.local or run: fly secrets list", secret),
            ));
        }
    }

    // Also validate SSM paths
    let ssm_result = validate_ssm_paths(env)?;
    result.ssm_paths = ssm_result.ssm_paths;
    result.warnings.extend(ssm_result.warnings);

    // Real SSM parameter checks if requested
    if ssm_check {
        let env_str = if let Some(e) = env {
            e.to_string()
        } else if let Ok(e) = std::env::var("ENVIRONMENT") {
            e
        } else if let Ok(e) = std::env::var("SSM_ENV") {
            e
        } else {
            "dev".to_string()
        };

        match SsmService::new(env_str.clone(), None).await {
            Ok(ssm_service) => {
                let env_normalized = normalize_env_for_ssm(&env_str);
                let expected_paths = vec![
                    (
                        "DATABASE_URL",
                        format!("/leadsnebula/{}/rust/db/connection_url", env_normalized),
                    ),
                    (
                        "REDIS_URL",
                        format!("/leadsnebula/{}/rust/redis/connection_url", env_normalized),
                    ),
                    (
                        "JWT_SECRET",
                        format!("/leadsnebula/{}/rust/auth/jwt_secret", env_normalized),
                    ),
                    (
                        "ENCRYPTION_KEY",
                        format!("/leadsnebula/{}/rust/encryption/primary_v1", env_normalized),
                    ),
                ];

                for (_name, path) in expected_paths {
                    match ssm_service.get_parameter(&path, false).await {
                        Ok(Some(_)) => {
                            // Parameter exists
                        }
                        Ok(None) => {
                            result.missing_ssm_params.push(path.clone());
                            result.warnings.push(ErrorMessage::with_remediation(
                                format!("SSM parameter not found: {}", path),
                                format!(
                                    "Create parameter in SSM or run: aws ssm put-parameter --name {} --value <value> --type String",
                                    path
                                ),
                            ));
                        }
                        Err(e) => {
                            // Permission denied or other error
                            if e.to_string().contains("AccessDenied") {
                                result.warnings.push(ErrorMessage::with_remediation(
                                    format!("Access denied to SSM parameter: {}", path),
                                    "Check AWS credentials and IAM permissions".to_string(),
                                ));
                            } else {
                                result.warnings.push(ErrorMessage::new(format!(
                                    "Failed to check SSM parameter {}: {}",
                                    path, e
                                )));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // AWS credentials missing or other error
                result.warnings.push(ErrorMessage::with_remediation(
                    format!("Failed to create SSM service: {}", e),
                    "Set AWS credentials (AWS_PROFILE or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY) or skip --ssm-check".to_string(),
                ));
            }
        }
    }

    Ok(result)
}

fn validate_github_workflow(path: &PathBuf) -> Result<ValidationResult> {
    let mut result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read workflow file: {}", path.display()))?;

    let yaml: YamlValue = serde_yaml::from_str(&content).context("Failed to parse YAML")?;

    // Check if this is a composite action (has runs.using: composite)
    let is_composite_action = yaml
        .get("runs")
        .and_then(|r| r.get("using"))
        .and_then(|u| u.as_str())
        .map(|u| u == "composite")
        .unwrap_or(false);

    // For composite actions, validate inputs structure
    if is_composite_action {
        if let Some(inputs) = yaml.get("inputs") {
            // If inputs key exists, it must be a mapping (object), not null
            if inputs.is_null() {
                result.errors.push(ErrorMessage::with_remediation(
                    format!(
                        "Composite action '{}' has null 'inputs' - must be a mapping or omitted",
                        path.display()
                    ),
                    "Remove 'inputs:' line if no inputs needed, or add 'inputs: {}' for empty mapping".to_string(),
                ));
                result.valid = false;
            } else if inputs.as_mapping().is_none() {
                result.errors.push(ErrorMessage::with_remediation(
                    format!(
                        "Composite action '{}' has invalid 'inputs' type - must be a mapping",
                        path.display()
                    ),
                    "Change 'inputs:' to a valid YAML mapping (object) or remove it".to_string(),
                ));
                result.valid = false;
            }
        }
    }

    // Check if workflow pushes to GHCR but doesn't have packages: write permission
    let mut pushes_to_ghcr = false;
    let mut has_packages_write = false;

    // Check for GHCR references in workflow content
    if content.contains("ghcr.io") {
        pushes_to_ghcr = true;
    }

    // Check for packages: write permission at workflow level
    if let Some(permissions) = yaml.get("permissions") {
        if let Some(packages) = permissions.get("packages") {
            if let Some(level) = packages.as_str() {
                if level == "write" {
                    has_packages_write = true;
                }
            }
        }
    }

    // Also check job-level permissions
    if let Some(jobs) = yaml.get("jobs").and_then(|j| j.as_mapping()) {
        for (_, job) in jobs {
            if let Some(permissions) = job.get("permissions") {
                if let Some(packages) = permissions.get("packages") {
                    if let Some(level) = packages.as_str() {
                        if level == "write" {
                            has_packages_write = true;
                        }
                    }
                }
            }
        }
    }

    if pushes_to_ghcr && !has_packages_write {
        result.warnings.push(ErrorMessage::with_remediation(
            "Workflow pushes to GHCR but missing packages: write permission".to_string(),
            "Add 'permissions: { packages: write }' to workflow or job level".to_string(),
        ));
    }

    // Check for invalid cargo llvm-cov options
    if let Some(jobs) = yaml.get("jobs").and_then(|j| j.as_mapping()) {
        for (job_name, job) in jobs {
            if let Some(steps) = job.get("steps").and_then(|s| s.as_sequence()) {
                for step in steps {
                    if let Some(run) = step.get("run").and_then(|r| r.as_str()) {
                        if run.contains("cargo llvm-cov") {
                            // Check for invalid options
                            if !run.contains("--lcov") || !run.contains("--output-path") {
                                result.warnings.push(ErrorMessage::with_remediation(
                                    format!(
                                        "Job '{}' uses cargo llvm-cov without --lcov --output-path",
                                        job_name.as_str().unwrap_or("unknown")
                                    ),
                                    "Use: cargo llvm-cov --lcov --output-path lcov.info"
                                        .to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

fn validate_cargo_duplicates() -> Result<ValidationResult> {
    let mut result = ValidationResult {
        valid: true,
        ..Default::default()
    };

    // Run cargo tree --duplicates to get duplicate packages
    let output = Command::new("cargo")
        .args(["tree", "--duplicates", "--format", "{p} {v}"])
        .output()
        .context("Failed to run cargo tree --duplicates")?;

    if !output.status.success() {
        result.warnings.push(ErrorMessage::new(
            "Failed to run cargo tree --duplicates".to_string(),
        ));
        return Ok(result);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        // No duplicates found
        return Ok(result);
    }

    // Parse output: group by package name, collect versions
    let mut duplicates: HashMap<String, Vec<String>> = HashMap::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let package = parts[0].to_string();
            let version = parts[1].to_string();
            duplicates.entry(package).or_default().push(version);
        }
    }

    // Create warnings for each duplicate package
    for (package, versions) in duplicates {
        let unique_versions: Vec<String> = versions
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if unique_versions.len() > 1 {
            result.warnings.push(ErrorMessage::with_remediation(
                format!(
                    "Package '{}' has multiple versions: {}",
                    package,
                    unique_versions.join(", ")
                ),
                "Run 'cargo tree --duplicates' to see dependency tree, then update Cargo.toml to use single version".to_string(),
            ));
        }
    }

    Ok(result)
}
