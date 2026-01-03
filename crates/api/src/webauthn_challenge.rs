use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;
use webauthn_rs::prelude::PasskeyRegistration;

#[derive(Clone)]
struct ChallengeData {
    #[allow(dead_code)] // Used internally but not accessed directly
    challenge: String,
    #[allow(dead_code)] // Used internally but not accessed directly
    user_id: Uuid,
    #[allow(dead_code)] // Used internally but not accessed directly
    created_at: Instant,
}

/// In-memory challenge storage for WebAuthn registration
/// Challenges expire after 5 minutes
pub struct ChallengeStore {
    #[allow(dead_code)] // Used internally via methods
    challenges: Mutex<HashMap<String, ChallengeData>>,
    registration_states: Mutex<HashMap<String, (PasskeyRegistration, Uuid, Instant)>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self {
            challenges: Mutex::new(HashMap::new()),
            registration_states: Mutex::new(HashMap::new()),
        }
    }

    #[allow(dead_code)] // Prepared for future WebAuthn implementation
    pub async fn store_challenge(&self, token: String, challenge: String, user_id: Uuid) {
        let mut challenges = self.challenges.lock().await;
        challenges.insert(
            token,
            ChallengeData {
                challenge,
                user_id,
                created_at: Instant::now(),
            },
        );
    }

    #[allow(dead_code)] // Prepared for future WebAuthn implementation
    pub async fn get_challenge(&self, token: &str, user_id: Uuid) -> Option<String> {
        let mut challenges = self.challenges.lock().await;

        // Clean up expired challenges (older than 5 minutes)
        challenges.retain(|_, data| data.created_at.elapsed() < Duration::from_secs(300));

        if let Some(data) = challenges.get(token) {
            // Verify user ID matches
            if data.user_id == user_id && data.created_at.elapsed() < Duration::from_secs(300) {
                let challenge = data.challenge.clone();
                challenges.remove(token);
                return Some(challenge);
            }
        }
        None
    }

    pub async fn store_registration_state(
        &self,
        token: String,
        reg_state: PasskeyRegistration,
        user_id: Uuid,
    ) {
        let mut states = self.registration_states.lock().await;
        states.insert(token, (reg_state, user_id, Instant::now()));
    }

    pub async fn get_registration_state(
        &self,
        token: &str,
        user_id: Uuid,
    ) -> Option<PasskeyRegistration> {
        let mut states = self.registration_states.lock().await;

        // Clean up expired states (older than 5 minutes)
        states.retain(|_, (_, _, created_at)| created_at.elapsed() < Duration::from_secs(300));

        if let Some((reg_state, stored_user_id, created_at)) = states.get(token) {
            // Verify user ID matches and not expired
            if *stored_user_id == user_id && created_at.elapsed() < Duration::from_secs(300) {
                let reg_state = reg_state.clone();
                states.remove(token);
                return Some(reg_state);
            }
        }
        None
    }
}

impl Default for ChallengeStore {
    fn default() -> Self {
        Self::new()
    }
}
