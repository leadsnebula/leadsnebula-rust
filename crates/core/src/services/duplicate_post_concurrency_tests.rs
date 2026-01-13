// Concurrency tests for duplicate post atomicity
// Tests that atomic claim semantics prevent double-selling

#[cfg(test)]
mod duplicate_post_concurrency_tests {
    use sqlx::PgPool;
    use std::sync::Arc;
    use tokio::sync::Barrier;
    use uuid::Uuid;

    async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .ok_or_else(|| "DATABASE_URL not set".to_string())?;

        use sqlx::postgres::PgPoolOptions;
        use tokio::time::Duration;

        let pool = PgPoolOptions::new()
            .max_connections(10) // More connections for concurrency test
            .acquire_timeout(Duration::from_secs(10))
            .connect(&database_url)
            .await?;

        Ok(pool)
    }

    async fn setup_test_lead(pool: &PgPool) -> (Uuid, String) {
        // Create instance_user
        let instance_user_id = Uuid::new_v4();
        let unique_email = format!("test_user_{}@test.invalid", Uuid::new_v4());
        let password_hash = "hashed_password";

        sqlx::query(
            r#"
            INSERT INTO instance_users (id, email, encrypted_password, status, confirmed_at, created_at, updated_at)
            VALUES ($1, $2, $3, 'active', NOW(), NOW(), NOW())
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(instance_user_id)
        .bind(&unique_email)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();

        // Create instance
        let instance_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO instances (id, name, payment_status, instance_user_id, created_at, updated_at)
             VALUES ($1, 'Test Instance', 'active', $2, NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(instance_id)
        .bind(instance_user_id)
        .execute(pool)
        .await
        .unwrap();

        // Create publisher
        let publisher_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO publishers (id, instance_id, name, email, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Publisher', $3, 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(publisher_id)
        .bind(instance_id)
        .bind(&format!("publisher_{}@test.invalid", Uuid::new_v4()))
        .execute(pool)
        .await
        .unwrap();

        // Create vertical
        let vertical_id = Uuid::new_v4();
        let vertical_slug = format!("test-vertical-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO verticals (id, slug, name, status, created_at, updated_at)
             VALUES ($1, $2, 'Test Vertical', 'active', NOW(), NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(vertical_id)
        .bind(&vertical_slug)
        .execute(pool)
        .await
        .unwrap();

        // Create lead with promise_id
        let lead_uuid = Uuid::new_v4();
        let promise_id = format!("PROMISE_{}", Uuid::new_v4());

        sqlx::query(
            r#"
            INSERT INTO leads (uuid, event_id, publisher_id, vertical_id, request_type, strategy, status, promise_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, 'ping', 'default', 'ping_accepted', $5, NOW(), NOW())
            "#,
        )
        .bind(lead_uuid)
        .bind(format!("evt_{}", Uuid::new_v4()))
        .bind(publisher_id)
        .bind(vertical_id)
        .bind(&promise_id)
        .execute(pool)
        .await
        .unwrap();

        (lead_uuid, promise_id)
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_duplicate_post_atomicity() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (lead_uuid, promise_id) = setup_test_lead(&pool).await;

        // Simulate concurrent post attempts
        let num_concurrent = 10;
        let barrier = Arc::new(Barrier::new(num_concurrent));
        let pool_arc = Arc::new(pool);

        let mut handles = vec![];

        for i in 0..num_concurrent {
            let pool_clone = pool_arc.clone();
            let barrier_clone = barrier.clone();
            let promise_id_clone = promise_id.clone();
            let lead_uuid_clone = lead_uuid;

            let handle = tokio::spawn(async move {
                // Wait for all threads to be ready
                barrier_clone.wait().await;

                // Attempt atomic claim
                let inprog_token = format!("INPROG_{}", uuid::Uuid::new_v4());
                let claim_result = sqlx::query_scalar::<_, Option<Uuid>>(
                    "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
                )
                .bind(&inprog_token)
                .bind(lead_uuid_clone)
                .bind(&promise_id_clone)
                .fetch_optional(&*pool_clone)
                .await;

                (i, claim_result)
            });

            handles.push(handle);
        }

        // Collect results
        let mut successful_claims = 0;
        for handle in handles {
            let (thread_id, result) = handle.await.unwrap();
            match result {
                Ok(Some(_)) => {
                    successful_claims += 1;
                    eprintln!("Thread {} successfully claimed", thread_id);
                }
                Ok(None) => {
                    eprintln!("Thread {} failed to claim (already claimed)", thread_id);
                }
                Err(e) => {
                    eprintln!("Thread {} error: {}", thread_id, e);
                }
            }
        }

        // Only one thread should successfully claim
        assert_eq!(
            successful_claims, 1,
            "Only one concurrent post attempt should succeed. Got {} successful claims",
            successful_claims
        );

        // Verify final state
        let final_post_id: Option<String> =
            sqlx::query_scalar("SELECT post_id FROM leads WHERE uuid = $1")
                .bind(lead_uuid)
                .fetch_one(&*pool_arc)
                .await
                .unwrap();

        assert!(
            final_post_id.is_some() && !final_post_id.as_ref().unwrap().is_empty(),
            "Lead should have post_id set"
        );
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_duplicate_post_with_different_promise_ids() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (lead_uuid, promise_id) = setup_test_lead(&pool).await;
        let wrong_promise_id = "WRONG_PROMISE_ID";

        // Attempt with correct promise_id
        let inprog_token1 = format!("INPROG_{}", uuid::Uuid::new_v4());
        let claim1 = sqlx::query_scalar::<_, Option<Uuid>>(
            "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
        )
        .bind(&inprog_token1)
        .bind(lead_uuid)
        .bind(&promise_id)
        .fetch_optional(&pool)
        .await
        .unwrap();

        // Attempt with wrong promise_id (should fail)
        let inprog_token2 = format!("INPROG_{}", uuid::Uuid::new_v4());
        let claim2 = sqlx::query_scalar::<_, Option<Uuid>>(
            "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
        )
        .bind(&inprog_token2)
        .bind(lead_uuid)
        .bind(wrong_promise_id)
        .fetch_optional(&pool)
        .await
        .unwrap();

        assert!(claim1.is_some(), "Correct promise_id should claim");
        assert!(claim2.is_none(), "Wrong promise_id should not claim");
    }

    #[tokio::test]
    #[ignore] // Requires DATABASE_URL
    async fn test_duplicate_post_after_already_posted() {
        let pool = match create_test_pool().await {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test - DATABASE_URL not set");
                return;
            }
        };

        let (lead_uuid, promise_id) = setup_test_lead(&pool).await;

        // First claim should succeed
        let inprog_token1 = format!("INPROG_{}", uuid::Uuid::new_v4());
        let claim1 = sqlx::query_scalar::<_, Option<Uuid>>(
            "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
        )
        .bind(&inprog_token1)
        .bind(lead_uuid)
        .bind(&promise_id)
        .fetch_optional(&pool)
        .await
        .unwrap();

        assert!(claim1.is_some(), "First claim should succeed");

        // Second claim should fail (post_id already set)
        let inprog_token2 = format!("INPROG_{}", uuid::Uuid::new_v4());
        let claim2 = sqlx::query_scalar::<_, Option<Uuid>>(
            "UPDATE leads SET post_id = $1 WHERE uuid = $2 AND (post_id IS NULL OR post_id = '') AND promise_id = $3 AND created_at >= NOW() - INTERVAL '10 minutes' RETURNING uuid",
        )
        .bind(&inprog_token2)
        .bind(lead_uuid)
        .bind(&promise_id)
        .fetch_optional(&pool)
        .await
        .unwrap();

        assert!(
            claim2.is_none(),
            "Second claim should fail (already posted)"
        );
    }
}
