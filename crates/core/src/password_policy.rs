pub fn validate_password(password: &str) -> anyhow::Result<()> {
    if password.len() < 8 {
        return Err(anyhow::anyhow!("Password must be at least 8 characters"));
    }

    if password.len() > 128 {
        return Err(anyhow::anyhow!("Password must be at most 128 characters"));
    }

    // Check for at least one uppercase letter
    if !password.chars().any(|c| c.is_uppercase()) {
        return Err(anyhow::anyhow!(
            "Password must contain at least one uppercase letter"
        ));
    }

    // Check for at least one lowercase letter
    if !password.chars().any(|c| c.is_lowercase()) {
        return Err(anyhow::anyhow!(
            "Password must contain at least one lowercase letter"
        ));
    }

    // Check for at least one number
    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err(anyhow::anyhow!("Password must contain at least one number"));
    }

    Ok(())
}
