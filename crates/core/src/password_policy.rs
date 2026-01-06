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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_validate_password_valid() {
        assert!(validate_password("ValidPass123").is_ok());
        assert!(validate_password("AnotherValid1").is_ok());
        assert!(validate_password("Complex!@#Pass123").is_ok());
    }

    #[test]
    fn test_validate_password_too_short() {
        let result = validate_password("Short1");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at least 8 characters"));
    }

    #[test]
    fn test_validate_password_too_long() {
        let long_password = "a".repeat(129) + "A1";
        let result = validate_password(&long_password);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at most 128 characters"));
    }

    #[test]
    fn test_validate_password_no_uppercase() {
        let result = validate_password("lowercase123");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("uppercase letter"));
    }

    #[test]
    fn test_validate_password_no_lowercase() {
        let result = validate_password("UPPERCASE123");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lowercase letter"));
    }

    #[test]
    fn test_validate_password_no_number() {
        let result = validate_password("NoNumberHere");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("number"));
    }

    #[test]
    fn test_validate_password_minimum_valid() {
        // Exactly 8 chars with all requirements
        assert!(validate_password("Valid123").is_ok());
    }

    #[test]
    fn test_validate_password_maximum_valid() {
        // Exactly 128 chars with all requirements
        let max_password = "A".repeat(125) + "a1";
        assert!(validate_password(&max_password).is_ok());
    }

    // Property-based test: valid passwords should pass
    proptest! {
        #[test]
        fn test_validate_password_property(
            uppercase in "[A-Z]",
            lowercase in "[a-z]",
            number in "[0-9]",
            middle in "[a-zA-Z0-9!@#$%^&*()_+\\-=\\[\\]{};':\"\\\\|,.<>\\/?]{0,120}"
        ) {
            let password = format!("{}{}{}{}", uppercase, lowercase, number, middle);
            if password.len() >= 8 && password.len() <= 128 {
                // Check if it has all requirements
                let has_upper = password.chars().any(|c| c.is_uppercase());
                let has_lower = password.chars().any(|c| c.is_lowercase());
                let has_digit = password.chars().any(|c| c.is_ascii_digit());

                if has_upper && has_lower && has_digit {
                    prop_assert!(validate_password(&password).is_ok());
                }
            }
        }
    }
}
