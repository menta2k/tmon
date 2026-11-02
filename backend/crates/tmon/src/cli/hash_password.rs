//! `tmon hash-password`: stdin → argon2id PHC string.

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, SaltString};
use std::io::{BufRead, IsTerminal};

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash(password: &str) -> anyhow::Result<String> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        anyhow::bail!("password must be at least {MIN_PASSWORD_LEN} characters");
    }
    let mut salt_bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    let h = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash: {e}"))?;
    Ok(h.to_string())
}

pub fn run() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("enter password, then press Enter:");
    }
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    println!("{}", hash(line.trim_end_matches(['\r', '\n']))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::PasswordVerifier;
    use argon2::password_hash::PasswordHash;

    #[test]
    fn hashes_and_verifies() {
        let h = hash("changeme").unwrap();
        assert!(h.starts_with("$argon2id$"));
        let parsed = PasswordHash::new(&h).unwrap();
        assert!(
            Argon2::default()
                .verify_password(b"changeme", &parsed)
                .is_ok()
        );
        assert!(hash("short").is_err());
    }
}
