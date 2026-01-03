use leadsnebula_core::PasswordHelper;
use std::env;

fn main() {
    let password = env::args().nth(1).expect("Password required");
    match PasswordHelper::hash_password(&password) {
        Ok(hash) => println!("{}", hash),
        Err(e) => eprintln!("Error: {}", e),
    }
}
