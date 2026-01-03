use leadsnebula_core::PasswordHelper;
use std::env;

fn main() {
    let password = env::args().nth(1).expect("Password required");
    let hash = env::args().nth(2).expect("Hash required");

    match PasswordHelper::verify_password(&password, &hash) {
        Ok(valid) => {
            if valid {
                println!("✅ Password verification: SUCCESS");
            } else {
                println!("❌ Password verification: FAILED");
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
