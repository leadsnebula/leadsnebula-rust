use anyhow::Result;
use leadsnebula_core::services::database::create_pool;
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = create_pool(&database_url).await?;

    // Check if instance_users table exists
    let table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'instance_users')",
    )
    .fetch_one(&pool)
    .await?;

    println!("Instance users table exists: {}\n", table_exists);

    if !table_exists {
        println!("⚠️  instance_users table does not exist. Need to create it.");
        return Ok(());
    }

    // Check for user
    let email = "boris@leadsnebula.com";
    type UserRow = (uuid::Uuid, String, String, Option<String>, Option<String>);
    let user: Option<UserRow> = sqlx::query(
        "SELECT id, email, encrypted_password, first_name, last_name FROM instance_users WHERE LOWER(email) = LOWER($1) LIMIT 1"
    )
    .bind(email)
    .map(|row: sqlx::postgres::PgRow| {
        (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
    })
    .fetch_optional(&pool)
    .await?;

    if let Some((id, email, pwd_hash, first_name, last_name)) = user {
        println!("✓ User found:");
        println!("  ID: {}", id);
        println!("  Email: {}", email);
        println!(
            "  First Name: {}",
            first_name.unwrap_or_else(|| "NULL".to_string())
        );
        println!(
            "  Last Name: {}",
            last_name.unwrap_or_else(|| "NULL".to_string())
        );
        println!(
            "  Password Hash: {}...",
            &pwd_hash[..20.min(pwd_hash.len())]
        );

        // Check status and confirmed_at
        let (status, confirmed_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query("SELECT status, confirmed_at FROM instance_users WHERE id = $1")
                .bind(id)
                .map(|row: sqlx::postgres::PgRow| (row.get(0), row.get(1)))
                .fetch_one(&pool)
                .await?;

        println!("  Status: {}", status);
        println!("  Confirmed: {}", confirmed_at.is_some());

        if status != "active" {
            println!("\n⚠️  User status is '{}', not 'active'", status);
        }
        if confirmed_at.is_none() {
            println!("\n⚠️  User is not confirmed (confirmed_at is NULL)");
        }
    } else {
        println!("✗ User not found: {}", email);
    }

    Ok(())
}
