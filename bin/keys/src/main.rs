use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use clap::{arg, Parser};
use eyre::{eyre, Result};
use pbkdf2::pbkdf2_hmac;
use rand::{rng, Rng};
use sha2::Sha256;

/// Size of the salt used for key derivation (32 bytes)
const SALT_SIZE: usize = 32;
/// Size of the nonce used for AES-GCM encryption (12 bytes)
const NONCE_SIZE: usize = 12;
/// Size of the derived encryption key (32 bytes for AES-256)
const KEY_SIZE: usize = 32;
/// Number of PBKDF2 iterations for key derivation
const PBKDF2_ITERATIONS: u32 = 100_000;

/// Command-line interface for the keys utility
#[derive(Parser, Debug)]
enum Commands {
    /// Generate a cryptographically secure random password
    GeneratePassword,
    /// Encrypt a private key with a password
    Encrypt {
        /// The private key to encrypt (hex format, with or without 0x prefix)
        #[arg(short, long)]
        key: String,
    },
}

/// Encrypts a private key using AES-256-GCM with PBKDF2 key derivation
///
/// # Arguments
/// * `private_key` - The private key bytes to encrypt
/// * `pwd` - The password bytes to derive the encryption key from
///
/// # Returns
/// * `Result<Vec<u8>>` - The encrypted data containing salt, nonce, and ciphertext
///
/// # Format
/// The returned encrypted data has the following structure:
/// - Bytes 0-31: Salt (32 bytes)
/// - Bytes 32-43: Nonce (12 bytes)
/// - Bytes 44+: Ciphertext with authentication tag
///
/// # Security
/// - Uses AES-256-GCM for authenticated encryption
/// - Derives key using PBKDF2-HMAC-SHA256 with 100,000 iterations
/// - Generates random salt and nonce for each encryption
fn encrypt_key(private_key: Vec<u8>, pwd: Vec<u8>) -> Result<Vec<u8>> {
    // Generate random salt for key derivation
    let mut salt = [0u8; SALT_SIZE];
    rng().fill(&mut salt);

    // Derive encryption key using PBKDF2 with 100,000 iterations
    let mut derived_key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(&pwd, &salt, PBKDF2_ITERATIONS, &mut derived_key);

    // Generate random nonce for AES-GCM
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    // Create cipher and encrypt
    let cipher = Aes256Gcm::new_from_slice(&derived_key).map_err(|e| eyre!("Failed to create cipher: {}", e))?;

    let ciphertext = cipher.encrypt(&nonce, private_key.as_ref()).map_err(|e| eyre!("Encryption failed: {}", e))?;

    // Format: salt || nonce || ciphertext (includes auth tag)
    let mut result = Vec::with_capacity(SALT_SIZE + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypts data that was encrypted with `encrypt_key`
///
/// # Arguments
/// * `encrypted_data` - The encrypted data containing salt, nonce, and ciphertext
/// * `pwd` - The password bytes to derive the decryption key from
///
/// # Returns
/// * `Result<Vec<u8>>` - The decrypted private key bytes
///
/// # Security
/// - Verifies authentication tag to ensure data integrity
/// - Uses the same PBKDF2 parameters as encryption
fn decrypt_key(encrypted_data: &[u8], pwd: &[u8]) -> Result<Vec<u8>> {
    if encrypted_data.len() < SALT_SIZE + NONCE_SIZE {
        return Err(eyre!("Invalid encrypted data: too short"));
    }

    // Extract components
    let salt = &encrypted_data[..SALT_SIZE];
    let nonce = &encrypted_data[SALT_SIZE..SALT_SIZE + NONCE_SIZE];
    let ciphertext = &encrypted_data[SALT_SIZE + NONCE_SIZE..];

    // Derive same encryption key using PBKDF2
    let mut derived_key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(pwd, salt, PBKDF2_ITERATIONS, &mut derived_key);

    // Create cipher and decrypt
    let cipher = Aes256Gcm::new_from_slice(&derived_key).map_err(|e| eyre!("Failed to create cipher: {}", e))?;

    let nonce = Nonce::from_slice(nonce);
    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|e| eyre!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

fn main() -> Result<()> {
    let args = Commands::parse();
    match args {
        Commands::GeneratePassword => {
            // Use cryptographically secure RNG for password generation
            let mut random = rng();
            let pwd: Vec<u8> = (0..16).map(|_| random.random::<u8>()).collect();
            println!("{pwd:?}");
        }
        Commands::Encrypt { key } => {
            let pwd = kabu_types_entities::private::KEY_ENCRYPTION_PWD.to_vec();

            let private_key = hex::decode(key.strip_prefix("0x").unwrap_or(key.clone().as_str()))?;
            let encrypted_key = encrypt_key(private_key.clone(), pwd.clone())?;

            // Verify decryption with new method
            let decrypted_key = decrypt_key(&encrypted_key, &pwd)?;
            if decrypted_key == private_key {
                println!("Encrypted private key : {}", hex::encode(encrypted_key))
            } else {
                println!("Error encrypting private key");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let private_key = vec![0x42; 32];
        let password = b"test_password_123!";

        let encrypted = encrypt_key(private_key.clone(), password.to_vec()).unwrap();
        let decrypted = decrypt_key(&encrypted, password).unwrap();

        assert_eq!(decrypted, private_key);
    }

    #[test]
    fn test_wrong_password_fails() {
        let private_key = vec![0x42; 32];
        let password = b"correct_password";
        let wrong_password = b"wrong_password";

        let encrypted = encrypt_key(private_key.clone(), password.to_vec()).unwrap();
        let result = decrypt_key(&encrypted, wrong_password);

        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_data_fails() {
        let private_key = vec![0x42; 32];
        let password = b"test_password";

        let mut encrypted = encrypt_key(private_key, password.to_vec()).unwrap();
        // Tamper with the ciphertext
        let last_idx = encrypted.len() - 1;
        encrypted[last_idx] ^= 0xFF;

        let result = decrypt_key(&encrypted, password);
        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_randomness() {
        let private_key = vec![0xff; 32];
        let password = b"same_password";

        let encrypted1 = encrypt_key(private_key.clone(), password.to_vec()).unwrap();
        let encrypted2 = encrypt_key(private_key.clone(), password.to_vec()).unwrap();

        // Should produce different ciphertexts due to random salt/nonce
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same private key
        let decrypted1 = decrypt_key(&encrypted1, password).unwrap();
        let decrypted2 = decrypt_key(&encrypted2, password).unwrap();
        assert_eq!(decrypted1, private_key);
        assert_eq!(decrypted2, private_key);
    }
}
