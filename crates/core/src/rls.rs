use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RlsContext {
    pub user_id: Option<Uuid>,
    pub organization_id: Option<Uuid>,
}

impl RlsContext {
    pub fn new(user_id: Option<Uuid>, organization_id: Option<Uuid>) -> Self {
        Self {
            user_id,
            organization_id,
        }
    }

    pub fn from_user(user_id: Uuid) -> Self {
        Self {
            user_id: Some(user_id),
            organization_id: None,
        }
    }
}
