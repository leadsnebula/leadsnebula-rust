use anyhow::Result;
use clap::Parser;
use leadsnebula_core::services::database::create_pool;
use std::io::{self, Write};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "cleanup-instance-data")]
#[command(
    about = "Clean up all data for an instance (leads, publishers, buyers, campaigns, ping_trees) while preserving instance and instance_user"
)]
struct Args {
    #[arg(short, long, default_value = "boris@leadsnebula.com")]
    email: String,

    #[arg(
        long,
        help = "Also clean up test records (emails ending with @test.invalid, @example.com, etc.)"
    )]
    clean_test_data: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env.local first for local development (highest priority)
    let env_loaded = dotenvy::from_filename(".env.local").is_ok();
    if !env_loaded {
        let _ = dotenvy::dotenv();
    }

    let args = Args::parse();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    println!("Connecting to database...");
    let pool = create_pool(&database_url).await?;

    println!("{}", "=".repeat(80));
    println!(
        "WARNING: This will delete ALL data for instance user: {}",
        args.email
    );
    println!("This includes:");
    println!("  - All leads and related data (pings, posts, payloads, etc.)");
    println!("  - All publishers and api_keys");
    println!("  - All buyers and buyer-related data");
    println!("  - All campaigns");
    println!("  - All ping_trees");
    println!();
    println!("The instance and instance_user will be PRESERVED.");
    println!("{}", "=".repeat(80));
    println!();

    // Find the instance_user
    #[derive(sqlx::FromRow)]
    struct InstanceUserRow {
        id: Uuid,
        email: String,
        status: String,
    }

    let instance_user: Option<InstanceUserRow> = sqlx::query_as(
        "SELECT id, email, status::text FROM instance_users WHERE LOWER(email) = LOWER($1) LIMIT 1",
    )
    .bind(&args.email)
    .fetch_optional(&pool)
    .await?;

    let instance_user = match instance_user {
        Some(user) => user,
        None => {
            eprintln!(
                "❌ Error: Instance user with email '{}' not found.",
                args.email
            );
            std::process::exit(1);
        }
    };

    println!("Found instance user:");
    println!("  ID: {}", instance_user.id);
    println!("  Email: {}", instance_user.email);
    println!("  Status: {}", instance_user.status);
    println!();

    // Find all instances for this user
    #[derive(sqlx::FromRow)]
    struct InstanceRow {
        id: Uuid,
        name: String,
    }

    let instances: Vec<InstanceRow> =
        sqlx::query_as("SELECT id, name FROM instances WHERE instance_user_id = $1")
            .bind(instance_user.id)
            .fetch_all(&pool)
            .await?;

    if instances.is_empty() {
        println!("⚠️  No instances found for this user.");
        return Ok(());
    }

    println!("Found {} instance(s):", instances.len());
    for instance in &instances {
        println!("  - {} (ID: {})", instance.name, instance.id);
    }
    println!();

    // Collect all instance IDs
    let instance_ids: Vec<Uuid> = instances.iter().map(|i| i.id).collect();

    // Count records before deletion
    println!("Counting records to be deleted...");

    let lead_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM leads l 
         INNER JOIN publishers p ON l.publisher_id = p.id 
         WHERE p.instance_id = ANY($1)",
    )
    .bind(&instance_ids)
    .fetch_one(&pool)
    .await?;

    let publisher_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM publishers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let buyer_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM buyers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let campaign_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM campaigns WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let ping_tree_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ping_trees WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    println!();
    println!("Current record counts:");
    println!("  Leads: {}", lead_count);
    println!("  Publishers: {}", publisher_count);
    println!("  Buyers: {}", buyer_count);
    println!("  Campaigns: {}", campaign_count);
    println!("  Ping Trees: {}", ping_tree_count);
    println!();

    if lead_count == 0
        && publisher_count == 0
        && buyer_count == 0
        && campaign_count == 0
        && ping_tree_count == 0
    {
        println!("✅ No data to delete. Instance is already clean.");
        return Ok(());
    }

    print!("Are you sure you want to delete all this data? Type 'yes' to confirm: ");
    io::stdout().flush()?;
    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;
    let confirmation = confirmation.trim();

    if confirmation != "yes" {
        println!("Aborted. No data was deleted.");
        return Ok(());
    }

    println!();
    println!("Starting deletion process...");
    println!();

    // Start transaction for atomicity
    let mut tx = pool.begin().await?;

    // Step 1: Get all lead IDs for this instance
    let lead_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT l.uuid FROM leads l 
         INNER JOIN publishers p ON l.publisher_id = p.id 
         WHERE p.instance_id = ANY($1)",
    )
    .bind(&instance_ids)
    .fetch_all(&mut *tx)
    .await?;

    if !lead_ids.is_empty() {
        println!("Step 1: Deleting lead-related child records...");

        // Delete in batches to avoid memory issues
        const BATCH_SIZE: usize = 1000;
        for chunk in lead_ids.chunks(BATCH_SIZE) {
            // Delete child records that reference leads if the table exists
            let exists_lead_sales: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_sales")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_sales {
                sqlx::query("DELETE FROM lead_sales WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_revenues: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_revenues")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_revenues {
                sqlx::query("DELETE FROM lead_revenues WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_accounting: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_accounting")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_accounting {
                sqlx::query("DELETE FROM lead_accounting WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_audit_log: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_audit_log")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_audit_log {
                sqlx::query("DELETE FROM lead_audit_log WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_consents: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_consents")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_consents {
                sqlx::query("DELETE FROM lead_consents WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_ip_addresses: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_ip_addresses")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_ip_addresses {
                sqlx::query("DELETE FROM lead_ip_addresses WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_lead_retries: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("lead_retries")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_lead_retries {
                sqlx::query("DELETE FROM lead_retries WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_pulsar_decision_logs: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("pulsar_decision_logs")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_pulsar_decision_logs {
                sqlx::query("DELETE FROM pulsar_decision_logs WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_fbcapi_events: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("fbcapi_events")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_fbcapi_events {
                sqlx::query("DELETE FROM fbcapi_events WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_pii_access_logs: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("pii_access_logs")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_pii_access_logs {
                sqlx::query("DELETE FROM pii_access_logs WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            // Delete ping and post related records
            let exists_ping_payloads: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("ping_payloads")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_ping_payloads {
                sqlx::query("DELETE FROM ping_payloads WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_post_payloads: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("post_payloads")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_post_payloads {
                sqlx::query("DELETE FROM post_payloads WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            // Delete pings and posts
            let exists_pings: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("pings")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_pings {
                sqlx::query("DELETE FROM pings WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }

            let exists_posts: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
            )
            .bind("posts")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if exists_posts {
                sqlx::query("DELETE FROM posts WHERE lead_id = ANY($1)")
                    .bind(chunk)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        print!(".");
        io::stdout().flush()?;
    }
    println!(" ✅");

    // Step 2: Delete leads
    println!("Step 2: Deleting leads...");
    let mut deleted_leads = 0;
    loop {
        let batch: Vec<Uuid> = sqlx::query_scalar(
            "SELECT l.uuid FROM leads l 
             INNER JOIN publishers p ON l.publisher_id = p.id 
             WHERE p.instance_id = ANY($1) 
             LIMIT 1000",
        )
        .bind(&instance_ids)
        .fetch_all(&mut *tx)
        .await?;

        if batch.is_empty() {
            break;
        }

        let result = sqlx::query("DELETE FROM leads WHERE uuid = ANY($1)")
            .bind(&batch)
            .execute(&mut *tx)
            .await?;

        deleted_leads += result.rows_affected();
        print!(".");
        io::stdout().flush()?;
    }
    println!(" ✅ ({} leads deleted)", deleted_leads);

    // Step 3: Get campaign, ping_tree, and buyer IDs before deleting publishers/buyers
    let campaign_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM campaigns WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_all(&mut *tx)
            .await?;

    let ping_tree_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM ping_trees WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_all(&mut *tx)
            .await?;

    let buyer_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM buyers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_all(&mut *tx)
            .await?;

    // Step 4: Delete legacy_integrations (if table exists)
    println!("Step 4: Deleting legacy_integrations...");
    let mut deleted_legacy = 0u64;
    let exists_legacy_integrations: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("legacy_integrations")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);

    if exists_legacy_integrations && (!buyer_ids.is_empty() || !campaign_ids.is_empty()) {
        if !buyer_ids.is_empty() {
            let result = sqlx::query("DELETE FROM legacy_integrations WHERE buyer_id = ANY($1)")
                .bind(&buyer_ids)
                .execute(&mut *tx)
                .await?;
            deleted_legacy += result.rows_affected();
        }

        if !campaign_ids.is_empty() {
            let result = sqlx::query("DELETE FROM legacy_integrations WHERE campaign_id = ANY($1)")
                .bind(&campaign_ids)
                .execute(&mut *tx)
                .await?;
            deleted_legacy += result.rows_affected();
        }
    }
    println!(" ✅ ({} legacy_integrations deleted)", deleted_legacy);

    // Step 5: Delete ping_tree_campaigns
    println!("Step 5: Deleting ping_tree_campaigns...");
    let result = sqlx::query("DELETE FROM ping_tree_campaigns WHERE ping_tree_id = ANY($1)")
        .bind(&ping_tree_ids)
        .execute(&mut *tx)
        .await?;
    let deleted_ptc = result.rows_affected();
    println!(" ✅ ({} ping_tree_campaigns deleted)", deleted_ptc);

    // Step 6: Delete campaigns
    println!("Step 6: Deleting campaigns...");
    let result = sqlx::query("DELETE FROM campaigns WHERE id = ANY($1)")
        .bind(&campaign_ids)
        .execute(&mut *tx)
        .await?;
    let deleted_campaigns = result.rows_affected();
    println!(" ✅ ({} campaigns deleted)", deleted_campaigns);

    // Step 7: Delete ping_trees
    println!("Step 7: Deleting ping_trees...");
    let result = sqlx::query("DELETE FROM ping_trees WHERE id = ANY($1)")
        .bind(&ping_tree_ids)
        .execute(&mut *tx)
        .await?;
    let deleted_ping_trees = result.rows_affected();
    println!(" ✅ ({} ping_trees deleted)", deleted_ping_trees);

    // Step 8: Get publisher IDs
    let publisher_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM publishers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_all(&mut *tx)
            .await?;

    // Step 9: Delete api_keys (if table exists)
    println!("Step 9: Deleting api_keys...");
    let exists_api_keys: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("api_keys")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let deleted_api_keys = if exists_api_keys {
        let result = sqlx::query("DELETE FROM api_keys WHERE publisher_id = ANY($1)")
            .bind(&publisher_ids)
            .execute(&mut *tx)
            .await?;
        result.rows_affected()
    } else {
        0
    };
    println!(" ✅ ({} api_keys deleted)", deleted_api_keys);

    // Step 10: Delete buyer-related tables
    println!("Step 10: Deleting buyer-related records...");

    // Step 10: Delete buyer-related tables (if they exist)
    println!("Step 10: Deleting buyer-related records...");

    let exists_buyer_zip_lists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("buyer_zip_lists")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);

    let buyer_zip_list_ids: Vec<Uuid> = if exists_buyer_zip_lists {
        sqlx::query_scalar("SELECT id FROM buyer_zip_lists WHERE buyer_id = ANY($1)")
            .bind(&buyer_ids)
            .fetch_all(&mut *tx)
            .await?
    } else {
        Vec::new()
    };

    let deleted_zip_codes = if !buyer_zip_list_ids.is_empty() {
        let exists_buyer_zip_codes: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
        )
        .bind("buyer_zip_codes")
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(false);
        if exists_buyer_zip_codes {
            let result =
                sqlx::query("DELETE FROM buyer_zip_codes WHERE buyer_zip_list_id = ANY($1)")
                    .bind(&buyer_zip_list_ids)
                    .execute(&mut *tx)
                    .await?;
            result.rows_affected()
        } else {
            0
        }
    } else {
        0
    };

    let deleted_zip_lists = if exists_buyer_zip_lists {
        let result = sqlx::query("DELETE FROM buyer_zip_lists WHERE buyer_id = ANY($1)")
            .bind(&buyer_ids)
            .execute(&mut *tx)
            .await?;
        result.rows_affected()
    } else {
        0
    };

    let exists_bic: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("buyer_integration_credentials")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let deleted_bic = if exists_bic {
        let result =
            sqlx::query("DELETE FROM buyer_integration_credentials WHERE buyer_id = ANY($1)")
                .bind(&buyer_ids)
                .execute(&mut *tx)
                .await?;
        result.rows_affected()
    } else {
        0
    };

    let exists_bqc: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("buyer_qualification_configs")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let deleted_bqc = if exists_bqc {
        let result =
            sqlx::query("DELETE FROM buyer_qualification_configs WHERE buyer_id = ANY($1)")
                .bind(&buyer_ids)
                .execute(&mut *tx)
                .await?;
        result.rows_affected()
    } else {
        0
    };

    println!(" ✅ ({} zip_codes, {} zip_lists, {} integration_credentials, {} qualification_configs deleted)", 
             deleted_zip_codes, deleted_zip_lists, deleted_bic, deleted_bqc);

    // Step 11: Delete buyers (if table exists)
    println!("Step 11: Deleting buyers...");
    let exists_buyers: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("buyers")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let deleted_buyers = if exists_buyers {
        let result = sqlx::query("DELETE FROM buyers WHERE id = ANY($1)")
            .bind(&buyer_ids)
            .execute(&mut *tx)
            .await?;
        result.rows_affected()
    } else {
        0
    };
    println!(" ✅ ({} buyers deleted)", deleted_buyers);

    // Step 12: Delete publishers (if table exists)
    println!("Step 12: Deleting publishers...");
    let exists_publishers: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = $1)",
    )
    .bind("publishers")
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let deleted_publishers = if exists_publishers {
        let result = sqlx::query("DELETE FROM publishers WHERE id = ANY($1)")
            .bind(&publisher_ids)
            .execute(&mut *tx)
            .await?;
        result.rows_affected()
    } else {
        0
    };
    println!(" ✅ ({} publishers deleted)", deleted_publishers);

    // Commit transaction
    tx.commit().await?;

    println!();
    println!("✅ Cleanup complete!");
    println!();
    println!("Summary:");
    println!("  Leads deleted: {}", deleted_leads);
    println!("  Publishers deleted: {}", deleted_publishers);
    println!("  Buyers deleted: {}", deleted_buyers);
    println!("  Campaigns deleted: {}", deleted_campaigns);
    println!("  Ping Trees deleted: {}", deleted_ping_trees);
    println!();
    println!("✅ Instance and instance_user have been preserved.");
    println!("{}", "=".repeat(80));

    // Verify cleanup
    println!();
    println!("Verifying cleanup...");

    let remaining_leads: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM leads l 
         INNER JOIN publishers p ON l.publisher_id = p.id 
         WHERE p.instance_id = ANY($1)",
    )
    .bind(&instance_ids)
    .fetch_one(&pool)
    .await?;

    let remaining_publishers: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM publishers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let remaining_buyers: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM buyers WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let remaining_campaigns: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM campaigns WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    let remaining_ping_trees: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ping_trees WHERE instance_id = ANY($1)")
            .bind(&instance_ids)
            .fetch_one(&pool)
            .await?;

    println!("  Remaining Leads: {}", remaining_leads);
    println!("  Remaining Publishers: {}", remaining_publishers);
    println!("  Remaining Buyers: {}", remaining_buyers);
    println!("  Remaining Campaigns: {}", remaining_campaigns);
    println!("  Remaining Ping Trees: {}", remaining_ping_trees);

    if remaining_leads > 0
        || remaining_publishers > 0
        || remaining_buyers > 0
        || remaining_campaigns > 0
        || remaining_ping_trees > 0
    {
        println!();
        println!("⚠️  Warning: Some records remain. This may indicate orphaned records or foreign key issues.");
    } else {
        println!();
        println!("✅ All data has been successfully cleaned up. Instance is ready for fresh data.");
    }

    println!();
    println!(
        "✅ Instance user '{}' is still active.",
        instance_user.email
    );
    println!("✅ {} instance(s) preserved.", instances.len());
    println!("{}", "=".repeat(80));

    // Clean up test records if requested
    if args.clean_test_data {
        println!();
        println!("{}", "=".repeat(80));
        println!("🧹 Cleaning up test records...");
        println!("{}", "=".repeat(80));
        println!();

        let mut test_tx = pool.begin().await?;
        let mut test_deleted_count = 0u64;

        // Find and delete test instance_users (emails ending with @test.invalid, @example.com, etc.)
        let test_email_patterns = vec![
            "%@test.invalid",
            "%@example.com",
            "%test%@test.%",
            "test_%@%",
        ];

        for pattern in &test_email_patterns {
            // Find test instance_users, EXCLUDING the real user email
            let test_users: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM instance_users WHERE email LIKE $1 AND LOWER(email) != LOWER($2)",
            )
            .bind(pattern)
            .bind(&args.email) // Exclude the real user
            .fetch_all(&mut *test_tx)
            .await?;

            if !test_users.is_empty() {
                println!(
                    "Found {} test instance_users matching pattern '{}' (excluding {})",
                    test_users.len(),
                    pattern,
                    args.email
                );

                // Find instances for these users
                let test_instance_ids: Vec<Uuid> =
                    sqlx::query_scalar("SELECT id FROM instances WHERE instance_user_id = ANY($1)")
                        .bind(&test_users)
                        .fetch_all(&mut *test_tx)
                        .await?;

                if !test_instance_ids.is_empty() {
                    // Delete all related data for test instances (same cascade as main cleanup)
                    // This is a simplified version - in production you might want to reuse the cleanup logic
                    let _ = sqlx::query("DELETE FROM leads WHERE publisher_id IN (SELECT id FROM publishers WHERE instance_id = ANY($1))")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;

                    let _ = sqlx::query("DELETE FROM publishers WHERE instance_id = ANY($1)")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;

                    let _ = sqlx::query("DELETE FROM buyers WHERE instance_id = ANY($1)")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;

                    let _ = sqlx::query("DELETE FROM campaigns WHERE instance_id = ANY($1)")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;

                    let _ = sqlx::query("DELETE FROM ping_trees WHERE instance_id = ANY($1)")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;

                    let _ = sqlx::query("DELETE FROM instances WHERE id = ANY($1)")
                        .bind(&test_instance_ids)
                        .execute(&mut *test_tx)
                        .await?;
                }

                // Delete foreign key dependencies before deleting instance_users
                // Delete webauthn_credentials first (has FK to instance_users)
                let _ = sqlx::query("DELETE FROM webauthn_credentials WHERE instance_user_id = ANY($1) OR instance_user_id = ANY($1)")
                    .bind(&test_users)
                    .execute(&mut *test_tx)
                    .await?;

                // Delete user_otp_settings (has FK to instance_users)
                let _ =
                    sqlx::query("DELETE FROM user_otp_settings WHERE instance_user_id = ANY($1)")
                        .bind(&test_users)
                        .execute(&mut *test_tx)
                        .await?;

                // Now safe to delete test instance_users
                let deleted = sqlx::query("DELETE FROM instance_users WHERE id = ANY($1)")
                    .bind(&test_users)
                    .execute(&mut *test_tx)
                    .await?;

                test_deleted_count += deleted.rows_affected();
                println!(
                    "  ✅ Deleted {} test instance_users and related data",
                    deleted.rows_affected()
                );
            }
        }

        // Also clean up test publishers (both orphaned and those linked to test instances)
        // Delete ALL test publishers matching test email patterns, regardless of instance linkage
        let test_publisher_patterns = vec![
            "%@test.invalid",
            "%@example.com",
            "test_%@%",
            "publisher_%@%",
        ];

        for pattern in &test_publisher_patterns {
            // Find test publishers, excluding those linked to the real user's instances
            let test_publishers: Vec<Uuid> = sqlx::query_scalar(
                r#"
                SELECT p.id FROM publishers p
                WHERE p.email LIKE $1
                AND (p.instance_id IS NULL 
                     OR p.instance_id NOT IN (
                         SELECT id FROM instances 
                         WHERE instance_user_id IN (
                             SELECT id FROM instance_users 
                             WHERE LOWER(email) = LOWER($2)
                         )
                     ))
                "#,
            )
            .bind(pattern)
            .bind(&args.email) // Exclude publishers from real user's instances
            .fetch_all(&mut *test_tx)
            .await?;

            if !test_publishers.is_empty() {
                // Delete related data first
                let _ = sqlx::query("DELETE FROM leads WHERE publisher_id = ANY($1)")
                    .bind(&test_publishers)
                    .execute(&mut *test_tx)
                    .await?;

                let _ = sqlx::query("DELETE FROM api_keys WHERE publisher_id = ANY($1)")
                    .bind(&test_publishers)
                    .execute(&mut *test_tx)
                    .await?;

                // Delete test publishers
                let deleted = sqlx::query("DELETE FROM publishers WHERE id = ANY($1)")
                    .bind(&test_publishers)
                    .execute(&mut *test_tx)
                    .await?;
                test_deleted_count += deleted.rows_affected();
                println!(
                    "  ✅ Deleted {} test publishers matching pattern '{}'",
                    deleted.rows_affected(),
                    pattern
                );
            }
        }

        test_tx.commit().await?;

        if test_deleted_count > 0 {
            println!();
            println!(
                "✅ Test data cleanup complete! Deleted {} test records total.",
                test_deleted_count
            );
        } else {
            println!();
            println!("ℹ️  No test records found to clean up.");
        }
        println!("{}", "=".repeat(80));
    }

    Ok(())
}
