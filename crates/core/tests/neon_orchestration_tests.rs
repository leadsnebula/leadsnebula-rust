use std::process::Command;

// This test is opt-in because it interacts with a real Neon service.
// To run it locally, set the following env vars:
// TEST_EPHEMERAL_NEON_TESTS=true NEON_API_KEY=... NEON_PROJECT_ID=... TEST_RUN_ID=local-<anything>

#[test]
fn create_and_delete_neon_branch_is_noop_when_not_enabled() {
    if std::env::var("TEST_EPHEMERAL_NEON_TESTS").unwrap_or_default() != "true" {
        eprintln!(
            "Skipping Neon orchestration tests (set TEST_EPHEMERAL_NEON_TESTS=true to enable)"
        );
        return;
    }

    let project =
        std::env::var("NEON_PROJECT_ID").expect("NEON_PROJECT_ID must be set for these tests");
    let run_id = std::env::var("TEST_RUN_ID")
        .unwrap_or_else(|_| "test-run-".to_string() + &uuid::Uuid::new_v4().to_string());
    let branch = format!("ci-local-{}", run_id);

    // Create
    let out = Command::new("neonctl")
        .args([
            "branches",
            "create",
            "--name",
            &branch,
            "--project",
            &project,
        ])
        .output()
        .expect("failed to run neonctl branches create");
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Get connection string
    let conn = Command::new("neonctl")
        .args(["connection-string", &branch, "--project", &project])
        .output()
        .expect("failed to run neonctl connection-string");
    assert!(
        conn.status.success(),
        "connection-string failed: {}",
        String::from_utf8_lossy(&conn.stderr)
    );

    // Delete
    let out = Command::new("neonctl")
        .args(["branches", "delete", &branch, "--project", &project])
        .output()
        .expect("failed to run neonctl branches delete");
    assert!(
        out.status.success(),
        "delete failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
