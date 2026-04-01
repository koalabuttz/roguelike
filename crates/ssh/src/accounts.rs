use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Minimum allowed password length for new accounts.
const MIN_PASSWORD_LENGTH: usize = 8;

/// Stored account data (one JSON file per user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub password_hash: String,
    pub created: String,
    pub last_login: String,
}

/// Account storage backed by JSON files in a directory.
pub struct AccountStore {
    accounts_dir: PathBuf,
}

impl AccountStore {
    pub fn new(data_dir: &Path) -> Self {
        let accounts_dir = data_dir.join("accounts");
        let _ = std::fs::create_dir_all(&accounts_dir);
        Self { accounts_dir }
    }

    fn account_path(&self, username: &str) -> PathBuf {
        self.accounts_dir.join(format!("{}.json", username))
    }

    /// Check if a username is already registered.
    pub fn exists(&self, username: &str) -> bool {
        self.account_path(username).exists()
    }

    /// Register a new account. Returns an error if the username is taken
    /// or the password cannot be hashed.
    pub fn register(&self, username: &str, password: &str) -> Result<(), String> {
        validate_username(username)?;
        if password.len() < MIN_PASSWORD_LENGTH {
            return Err(format!(
                "Password must be at least {} characters.",
                MIN_PASSWORD_LENGTH
            ));
        }
        if self.exists(username) {
            return Err("Username already taken.".to_string());
        }

        let hash = hash_password(password)?;
        let now = chrono_now();
        let account = Account {
            password_hash: hash,
            created: now.clone(),
            last_login: now,
        };
        let json = serde_json::to_string_pretty(&account)
            .map_err(|e| format!("Failed to serialize account: {e}"))?;
        std::fs::write(self.account_path(username), json)
            .map_err(|e| format!("Failed to write account file: {e}"))?;
        Ok(())
    }

    /// Verify login credentials. Returns Ok(()) on success, Err with a
    /// user-facing message on failure.
    pub fn login(&self, username: &str, password: &str) -> Result<(), String> {
        let path = self.account_path(username);
        let json = std::fs::read_to_string(&path).map_err(|e| {
            tracing::debug!("Login failed for '{}': {}", username, e);
            "Invalid username or password.".to_string()
        })?;
        let mut account: Account =
            serde_json::from_str(&json).map_err(|_| "Corrupted account file.".to_string())?;

        verify_password(password, &account.password_hash)?;

        // Update last_login timestamp.
        account.last_login = chrono_now();
        if let Ok(updated_json) = serde_json::to_string_pretty(&account) {
            let _ = std::fs::write(&path, updated_json);
        }

        Ok(())
    }

    /// Count registered accounts (for lobby display).
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        std::fs::read_dir(&self.accounts_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                    .count()
            })
            .unwrap_or(0)
    }
}

/// Validate a username: 3-32 chars, alphanumeric + hyphen + underscore.
pub fn validate_username(username: &str) -> Result<(), String> {
    if username.len() < 3 {
        return Err("Username must be at least 3 characters.".to_string());
    }
    if username.len() > 32 {
        return Err("Username must be at most 32 characters.".to_string());
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "Username may only contain letters, digits, hyphens, and underscores.".to_string(),
        );
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("Password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, hash_str: &str) -> Result<(), String> {
    let parsed = PasswordHash::new(hash_str).map_err(|_| "Corrupted password hash.".to_string())?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| "Invalid username or password.".to_string())
}

/// Simple ISO 8601 timestamp without pulling in chrono.
fn chrono_now() -> String {
    // Use std::time for a basic UTC-ish timestamp.
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Basic formatting: days since epoch → approximate date
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since 1970-01-01
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (AccountStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path());
        (store, dir)
    }

    #[test]
    fn validate_username_valid() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("bob-123").is_ok());
        assert!(validate_username("a_b").is_ok());
        assert!(validate_username("abc").is_ok());
    }

    #[test]
    fn validate_username_too_short() {
        assert!(validate_username("ab").is_err());
        assert!(validate_username("").is_err());
    }

    #[test]
    fn validate_username_too_long() {
        let long = "a".repeat(33);
        assert!(validate_username(&long).is_err());
    }

    #[test]
    fn validate_username_invalid_chars() {
        assert!(validate_username("a b c").is_err());
        assert!(validate_username("a@b").is_err());
        assert!(validate_username("a/b").is_err());
        assert!(validate_username("a..b").is_err());
    }

    #[test]
    fn register_and_login() {
        let (store, _dir) = temp_store();
        store.register("alice", "password123").unwrap();
        assert!(store.exists("alice"));
        assert!(store.login("alice", "password123").is_ok());
    }

    #[test]
    fn login_wrong_password() {
        let (store, _dir) = temp_store();
        store.register("bob", "correct1").unwrap();
        let err = store.login("bob", "wrong111").unwrap_err();
        assert_eq!(err, "Invalid username or password.");
    }

    #[test]
    fn login_nonexistent_user() {
        let (store, _dir) = temp_store();
        let err = store.login("nobody", "password").unwrap_err();
        assert_eq!(err, "Invalid username or password.");
    }

    #[test]
    fn login_errors_are_identical() {
        let (store, _dir) = temp_store();
        store.register("alice", "password123").unwrap();
        let wrong_user = store.login("nobody", "password123").unwrap_err();
        let wrong_pass = store.login("alice", "wrongpass").unwrap_err();
        assert_eq!(
            wrong_user, wrong_pass,
            "Error messages must be identical to prevent enumeration"
        );
    }

    #[test]
    fn register_duplicate() {
        let (store, _dir) = temp_store();
        store.register("alice", "password1").unwrap();
        assert!(store.register("alice", "password2").is_err());
    }

    #[test]
    fn register_empty_password() {
        let (store, _dir) = temp_store();
        assert!(store.register("alice", "").is_err());
    }

    #[test]
    fn register_short_password() {
        let (store, _dir) = temp_store();
        assert!(store.register("alice", "short").is_err());
        assert!(store.register("alice", "1234567").is_err()); // 7 chars
        assert!(store.register("alice", "12345678").is_ok()); // 8 chars
    }

    #[test]
    fn count_accounts() {
        let (store, _dir) = temp_store();
        assert_eq!(store.count(), 0);
        store.register("alice", "password").unwrap();
        assert_eq!(store.count(), 1);
        store.register("bob", "password").unwrap();
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn chrono_now_format() {
        let ts = chrono_now();
        // Should look like "2024-01-15T12:34:56Z"
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
        assert_eq!(ts.chars().nth(4), Some('-'));
        assert_eq!(ts.chars().nth(7), Some('-'));
        assert_eq!(ts.chars().nth(10), Some('T'));
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is 19723 days after epoch
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }
}
