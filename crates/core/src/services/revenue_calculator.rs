use sqlx::PgPool;
use uuid::Uuid;

// Platform fee (hardcoded for MVP, configurable in Phase 2)
const PLATFORM_FEE: f64 = 0.0015;

#[derive(Debug, Clone)]
pub struct RevenueCalculation {
    pub gross_sale_price: f64,
    pub platform_fee: f64,
    pub publisher_payment: f64,
    pub admin_fee_amount: f64,
}

/// Calculate revenue splits for a sold lead
///
/// # Arguments
/// * `lead_value` - The price paid by the buyer (gross sale price)
/// * `revshare_percentage` - Optional percentage revshare (0-100)
/// * `revshare_flat_amount` - Optional flat amount revshare
/// * `publisher_id` - The original publisher who submitted the lead
/// * `instance_id` - The instance ID to identify the admin/broker
///
/// # Returns
/// RevenueCalculation struct with all calculated amounts
///
/// # Errors
/// Returns error if calculation fails or if both revshare types are provided
pub async fn calculate_revenue(
    _pool: &PgPool,
    lead_value: f64,
    revshare_percentage: Option<f64>,
    revshare_flat_amount: Option<f64>,
    _publisher_id: Uuid,
    _instance_id: Uuid,
) -> Result<RevenueCalculation, sqlx::Error> {
    // Validate inputs
    if lead_value <= 0.0 {
        return Err(sqlx::Error::RowNotFound);
    }

    // Platform fee is deducted first
    let platform_fee = PLATFORM_FEE;
    let after_platform_fee = lead_value - platform_fee;

    // Calculate publisher payment based on revshare type
    let publisher_payment = if let Some(percentage) = revshare_percentage {
        // Percentage-based: publisher gets (lead_value - platform_fee) * (percentage / 100)
        after_platform_fee * (percentage / 100.0)
    } else if let Some(flat) = revshare_flat_amount {
        // Flat amount: publisher gets (lead_value - platform_fee - flat)
        // But we need to ensure it's not negative
        (after_platform_fee - flat).max(0.0)
    } else {
        // Default: if neither provided, use 80% (shouldn't happen if defaults applied)
        after_platform_fee * 0.8
    };

    // Admin fee is the remainder
    let admin_fee_amount = after_platform_fee - publisher_payment;

    Ok(RevenueCalculation {
        gross_sale_price: lead_value,
        platform_fee,
        publisher_payment,
        admin_fee_amount,
    })
}

/// Create a lead_revenues record
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `lead_id` - The lead UUID
/// * `lead_sale_id` - Optional lead_sale UUID
/// * `calculation` - The revenue calculation result
/// * `publisher_id` - Original publisher
/// * `buyer_id` - Buyer who purchased
/// * `campaign_id` - Campaign that won
/// * `instance_id` - Instance to identify admin
///
/// # Returns
/// UUID of created lead_revenue record
///
/// # Errors
/// Returns error if insert fails or if record already exists (idempotency check)
#[allow(clippy::too_many_arguments)]
pub async fn create_lead_revenue(
    pool: &PgPool,
    lead_id: Uuid,
    lead_sale_id: Option<Uuid>,
    calculation: &RevenueCalculation,
    publisher_id: Uuid,
    buyer_id: Uuid,
    campaign_id: Uuid,
    _instance_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    // Idempotency check: don't create duplicate records
    if let Some(existing_id) =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM lead_revenues WHERE lead_id = $1 LIMIT 1")
            .bind(lead_id)
            .fetch_optional(pool)
            .await?
    {
        return Ok(existing_id);
    }

    // Get instance owner's publisher ID (admin_id)
    // For now, we'll use a placeholder - this needs to be determined based on instance structure
    // TODO: Query instance to get owner's publisher ID
    let admin_id: Option<Uuid> = None; // Will be set when instance structure is clear

    // Calculate admin fee percentage for record keeping
    let admin_fee_percentage = if calculation.gross_sale_price > 0.0 {
        Some((calculation.admin_fee_amount / calculation.gross_sale_price) * 100.0)
    } else {
        None
    };

    let revenue_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO lead_revenues (
            id, lead_id, lead_sale_id,
            gross_sale_price, buyer_payment, platform_fee,
            admin_fee_percentage, admin_fee_amount, publisher_payment,
            publisher_id, buyer_id, campaign_id, admin_id,
            payment_status, created_at, updated_at
        ) VALUES (
            $1, $2, $3,
            $4, $5, $6,
            $7, $8, $9,
            $10, $11, $12, $13,
            'pending', NOW(), NOW()
        )
        "#,
    )
    .bind(revenue_id)
    .bind(lead_id)
    .bind(lead_sale_id)
    .bind(calculation.gross_sale_price)
    .bind(calculation.gross_sale_price) // buyer_payment = gross_sale_price
    .bind(calculation.platform_fee)
    .bind(admin_fee_percentage)
    .bind(calculation.admin_fee_amount)
    .bind(calculation.publisher_payment)
    .bind(publisher_id)
    .bind(buyer_id)
    .bind(campaign_id)
    .bind(admin_id)
    .execute(pool)
    .await?;

    Ok(revenue_id)
}
