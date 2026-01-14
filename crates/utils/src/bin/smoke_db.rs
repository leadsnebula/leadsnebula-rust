use anyhow::Result;
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env.local or env");

    let pool = sqlx::PgPool::connect(&database_url).await?;

    let row = sqlx::query("SELECT 1 as v").fetch_one(&pool).await?;
    let v: i32 = row.get("v");
    println!("DB smoke test result: {}", v);

    Ok(())
}
