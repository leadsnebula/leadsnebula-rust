use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use leadsnebula_core::encryption::EncryptionService;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing API key encryption/decryption...");
    println!();

    // Generate a test API key
    let test_api_key = "pk_live_1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    println!("Original API key: {}", test_api_key);
    println!();

    // Create a test encryption key (32 bytes)
    // In production, this would come from SSM
    let mut test_key_bytes = vec![0u8; 32];
    test_key_bytes.copy_from_slice(
        &general_purpose::STANDARD.decode("lpwGapuOQsKHIkN5M5qZUHdQwjriLp5J68TWejHMIsI=")?,
    );

    // Initialize encryption service
    let encryption_service = EncryptionService::new(&test_key_bytes)?;
    println!("✅ Encryption service initialized");
    println!();

    // Encrypt the API key
    let encrypted = encryption_service.encrypt(test_api_key)?;
    println!("Encrypted (base64): {}", encrypted);
    println!("Encrypted length: {} bytes", encrypted.len());
    println!();

    // Decrypt the API key
    let decrypted = encryption_service.decrypt(&encrypted)?;
    println!("Decrypted API key: {}", decrypted);
    println!();

    // Verify they match
    if decrypted == test_api_key {
        println!("✅ SUCCESS: Encryption and decryption work correctly!");
        println!("   Original and decrypted keys match.");
    } else {
        println!("❌ FAILED: Decrypted key does not match original!");
        println!("   Original:  {}", test_api_key);
        println!("   Decrypted: {}", decrypted);
        return Err(anyhow::anyhow!("Encryption/decryption test failed"));
    }

    Ok(())
}
