use serde::{Deserialize, Serialize};
use sqlx::Type;

// Lead status enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "lead_status_enum", rename_all = "snake_case")]
pub enum LeadStatus {
    Processing,
    PingAccepted,
    Sold,
    Rejected,
    Timeout,
    Invalid,
    Error,
    /// Test leads (is_test = true): do not count toward revenue; exclude from reports.
    Test,
}

impl LeadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LeadStatus::Processing => "processing",
            LeadStatus::PingAccepted => "ping_accepted",
            LeadStatus::Sold => "sold",
            LeadStatus::Rejected => "rejected",
            LeadStatus::Timeout => "timeout",
            LeadStatus::Invalid => "invalid",
            LeadStatus::Error => "error",
            LeadStatus::Test => "test",
        }
    }
}

impl std::fmt::Display for LeadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Publisher status enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "publisher_status_enum", rename_all = "lowercase")]
pub enum PublisherStatus {
    Active,
    Inactive,
    Suspended,
}

impl PublisherStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PublisherStatus::Active => "active",
            PublisherStatus::Inactive => "inactive",
            PublisherStatus::Suspended => "suspended",
        }
    }
}

impl std::fmt::Display for PublisherStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Buyer status enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "buyer_status_enum", rename_all = "lowercase")]
pub enum BuyerStatus {
    Active,
    Incomplete,
    Inactive,
    Suspended,
}

impl BuyerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuyerStatus::Active => "active",
            BuyerStatus::Incomplete => "incomplete",
            BuyerStatus::Inactive => "inactive",
            BuyerStatus::Suspended => "suspended",
        }
    }
}

impl std::fmt::Display for BuyerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Campaign status enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "campaign_status_enum", rename_all = "lowercase")]
pub enum CampaignStatus {
    Active,
    Paused,
    Inactive,
}

impl CampaignStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CampaignStatus::Active => "active",
            CampaignStatus::Paused => "paused",
            CampaignStatus::Inactive => "inactive",
        }
    }
}

impl std::fmt::Display for CampaignStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Ping result enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "ping_result_enum", rename_all = "lowercase")]
pub enum PingResult {
    Accepted,
    Rejected,
    Timeout,
    Invalid,
    Error,
    Sold,
}

impl PingResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            PingResult::Accepted => "accepted",
            PingResult::Rejected => "rejected",
            PingResult::Timeout => "timeout",
            PingResult::Invalid => "invalid",
            PingResult::Error => "error",
            PingResult::Sold => "sold",
        }
    }
}

impl std::fmt::Display for PingResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Post result enum
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[sqlx(type_name = "post_result_enum", rename_all = "lowercase")]
pub enum PostResult {
    Sold,
    Rejected,
    Timeout,
    Invalid,
    Error,
}

impl PostResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostResult::Sold => "sold",
            PostResult::Rejected => "rejected",
            PostResult::Timeout => "timeout",
            PostResult::Invalid => "invalid",
            PostResult::Error => "error",
        }
    }
}

impl std::fmt::Display for PostResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
