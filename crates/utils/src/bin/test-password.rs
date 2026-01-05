use anyhow::Result;
use leadsnebula_core::auth::verify_password;
use leadsnebula_core::services::database::create_pool;
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenv::from_filename(".env.local");
    let _ = dotenv::dotenv();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = create_pool(&database_url).await?;

    let email = "boris@leadsnebula.com";
    let password = "s@W&3$P4EUMFp81%%RFU";

    let (hash, user_id): (String, uuid::Uuid) = sqlx::query(
        "SELECT encrypted_password, id FROM instance_users WHERE LOWER(email) = LOWER($1) LIMIT 1",
    )
    .bind(email)
    .map(|row: sqlx::postgres::PgRow| (row.get(0), row.get(1)))
    .fetch_one(&pool)
    .await?;

    println!("Testing password verification for user: {}", email);
    println!("User ID: {}", user_id);
    println!(
        "Hash prefix: {}...",
        &hash.chars().take(30).collect::<String>()
    );
    println!("Password to test: {}", password);

    match verify_password(password, &hash) {
        Ok(true) => {
            println!("✅ Password verification SUCCESSFUL!");
        }
        Ok(false) => {
            println!("❌ Password verification FAILED - password does not match");
        }
        Err(e) => {
            println!("❌ Password verification ERROR: {}", e);
        }
    }

    Ok(())
}
