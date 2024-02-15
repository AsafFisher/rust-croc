use aes::Aes256;
use aes_gcm::{
    aead::{Aead, OsRng},
    AeadCore, Aes256Gcm, AesGcm, KeyInit,
};
use anyhow::{Error, Result};
use pbkdf2::pbkdf2_hmac_array;
use rand::RngCore;
use sha2::Sha256;

#[derive(Clone)]
pub struct AesEncryptor {
    cipher: AesGcm<Aes256, aes_gcm::aead::consts::U12>,
    pub salt: [u8; 8],
}

impl AesEncryptor {
    /// Creates a new `EncryptionSession` with the provided session key.
    ///
    /// This function generates a new encryption session with a unique salt and a derived encryption key
    /// based on the provided session key using PBKDF2-HMAC-SHA256.
    ///
    /// # Arguments
    ///
    /// * `session_key` - A reference to a 32-byte session key.
    ///
    /// # Returns
    ///
    /// An `EncryptionSession` with a generated salt and derived encryption key.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_croc::crypto::aes::AesEncryptor;
    ///
    /// // Create a new EncryptionSession with a session key
    /// let session_key = [0u8; 32];
    /// let encryption_session = AesEncryptor::new(&session_key, None);
    /// ```
    pub fn new(session_key: &[u8; 32], salt: Option<[u8; 8]>) -> Self {
        // Generate a unique salt
        let mut rnd = rand::thread_rng();
        let salt = match salt {
            Some(salt) => {
                debug!("Got salt: {:x?}", hex::encode(salt));
                salt
            }
            None => {
                let mut salt = [0u8; 8];
                rnd.fill_bytes(&mut salt);
                debug!("Generated salt: {:x?}", hex::encode(salt));
                salt
            }
        };

        // Derive a strong key using PBKDF2-HMAC-SHA256
        let strong_key = pbkdf2_hmac_array::<Sha256, 32>(session_key, &salt, 100);
        debug!("Derived strong_key: {:x?}", hex::encode(&strong_key));

        // Create an AES-GCM cipher instance with the strong key
        let cipher = aes_gcm::Aes256Gcm::new((&strong_key).into());
        Self { cipher, salt }
    }

    /// Encrypts the provided data using AES-GCM.
    ///
    /// This function encrypts the input data using the AES-GCM algorithm with a randomly generated nonce (IV).
    ///
    /// # Arguments
    ///
    /// * `data` - The data to be encrypted.
    ///
    /// # Returns
    ///
    /// A `Result` containing the encrypted data as a vector of bytes, or an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_croc::crypto::aes::AesEncryptor;
    /// // Create a new EncryptionSession with a session key
    /// let session_key = [0u8; 32];
    /// let encryption_session = AesEncryptor::new(&session_key, None);
    ///
    /// // Encrypt data
    /// let data = b"Hello, world!";
    /// let encrypted_data = encryption_session.encrypt(data).unwrap();
    /// ```
    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Generate a random nonce for encryption
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // Encrypt the data
        let mut ciphertext = self
            .cipher
            .encrypt(&nonce, data.as_ref())
            .map_err(Error::msg)?;

        // Prepend the nonce to the ciphertext and return the result
        let mut full_cipher = nonce.to_vec();
        full_cipher.append(&mut ciphertext);

        // TODO: Remove unwrap
        Ok(full_cipher)
    }

    /// Decrypts the provided encrypted data using AES-GCM.
    ///
    /// This function decrypts the input data using the AES-GCM algorithm with the provided nonce (IV).
    ///
    /// # Arguments
    ///
    /// * `data` - The encrypted data to be decrypted.
    ///
    /// # Returns
    ///
    /// A `Result` containing the decrypted data as a vector of bytes, or an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_croc::crypto::aes::AesEncryptor;
    /// // Create a new EncryptionSession with a session key
    /// let session_key = [0u8; 32];
    /// let encryption_session = AesEncryptor::new(&session_key, None);
    ///
    /// // Encrypt data
    /// let data = b"Hello, world!";
    /// let encrypted_data = encryption_session.encrypt(data).unwrap();
    ///
    /// // Decrypt data
    /// let decrypted_data = encryption_session.decrypt(&encrypted_data).unwrap();
    ///
    /// assert_eq!(data.to_vec(), decrypted_data);
    /// ```
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Extract the nonce from the encrypted data (first 12 bytes)
        let nonce = &data[..12];

        // Decrypt the data
        let decrypted_data = self
            .cipher
            .decrypt(nonce.into(), &data[12..])
            .map_err(Error::msg)?;

        Ok(decrypted_data)
    }
}
