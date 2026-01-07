// Integration tests for API routes
// These tests verify API-related functionality

use leadsnebula_core::auth::JwtService;

#[tokio::test]
async fn test_jwt_service_integration() {
    let secret = "test_secret_key_for_jwt_encoding".to_string();
    let service = JwtService::new(secret);
    let user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
    let email = "test@example.com".to_string();

    let token = service.encode(user_id.clone(), email.clone()).unwrap();
    assert!(!token.is_empty());

    let claims = service.decode(&token).unwrap();
    assert_eq!(claims.user_id, user_id);
    assert_eq!(claims.email, email);
}

#[tokio::test]
async fn test_jwt_token_expiration() {
    let secret = "test_secret".to_string();
    let service = JwtService::new(secret);
    let user_id = "user123".to_string();
    let email = "test@example.com".to_string();

    let token = service.encode(user_id, email).unwrap();

    // Token should be decodable immediately
    let claims = service.decode(&token).unwrap();
    assert!(claims.exp > claims.iat);
    assert!(claims.exp - claims.iat >= 24 * 3600 - 1); // Should be ~24 hours
}

// Note: Health endpoint integration tests are covered by:
// 1. Pre-deployment validation in CI (tests Docker container startup and /live endpoint)
// 2. Manual testing via Postman collection
// 3. Fly.io health checks in production
//
// For unit-level testing of health endpoints, we would need to either:
// - Create a lib.rs that exports routes (but this conflicts with binary-only structure)
// - Use tower-test to test HTTP endpoints (more complex setup)
//
// The current approach (pre-deployment validation) is more practical and tests the
// actual deployment scenario.
