//! Time-based one-time password (TOTP) generation per RFC 4226/6238.
//!
//! Used to answer keyboard-interactive prompts that expect a one-time code.
//! The secret key is a base32-encoded shared secret. The same secret and
//! time step reproduce the same six-digit code. The code is computed from
//! the HMAC-SHA1 of the timestamp floor-divided by the step.
//!
//! TOTP secrets are treated as credentials: keep the key material out of
//! logs and never write it back to the user's configuration store.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha1::Sha1;

/// TOTP codes are typically generated for 30-second steps.
pub const DEFAULT_STEP_SECONDS: u64 = 30;

/// Standard six-digit TOTP code length.
pub const DEFAULT_CODE_DIGITS: u32 = 6;

/// Decode a base32-encoded secret (RFC 4648, no padding required).
pub fn decode_secret(input: &str) -> Result<Vec<u8>, &'static str> {
    let cleaned: String = input
        .chars()
        .filter(|c| {
            !c.is_ascii_whitespace() && (*c == '-' || *c == '_' || c.is_ascii_alphanumeric())
        })
        .collect();
    if cleaned.is_empty() {
        return Err("Enter a base32 secret key");
    }
    data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|_| "Could not decode the base32 secret")
}

fn code_from_counter(secret: &[u8], counter: u64, digits: u32) -> Result<String, &'static str> {
    let mut macer = match Hmac::<Sha1>::new_from_slice(secret) {
        Ok(macer) => macer,
        Err(_) => return Err("Invalid secret length for HMAC-SHA1"),
    };
    macer.update(&counter.to_be_bytes());
    let digest = macer.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((digest[offset] & 0x7f) as u32) << 24
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    let modulus = 10u32.pow(digits);
    Ok(format!(
        "{:0width$}",
        binary % modulus,
        width = digits as usize
    ))
}

pub struct TotpCompute {
    pub secret_bytes: Vec<u8>,
    pub step_seconds: u64,
    pub digits: u32,
}

impl TotpCompute {
    pub fn from_secret(secret: &str, step_seconds: u64, digits: u32) -> Result<Self, &'static str> {
        let secret_bytes = decode_secret(secret)?;
        Ok(Self {
            secret_bytes,
            step_seconds,
            digits,
        })
    }

    pub fn compute(&self, now: SystemTime, drift_steps: i64) -> Result<String, &'static str> {
        let elapsed = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::ZERO);
        let epoch_seconds = elapsed.as_secs() as i64;
        let effective_epoch = epoch_seconds + drift_steps * self.step_seconds as i64;
        let counter = (effective_epoch.max(0) as u64) / self.step_seconds;
        code_from_counter(&self.secret_bytes, counter, self.digits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 reference vector (T = 59, SHA1, 6 digits -> 287082).
    const SHA1_HEX: &str = "3132333435363738393031323334353637383930";
    const SHA1_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn totp_matches_rfc6238_vectors() {
        // RFC 6238 SHA-1 vector at time T=59 (counter = 59//30 = 1).
        let secret = from_hex(SHA1_HEX);
        let eight = code_from_counter(&secret, 1, 8).unwrap();
        assert_eq!(eight, "94287082");
        let six = code_from_counter(&secret, 1, 6).unwrap();
        assert_eq!(six, "287082");
    }

    #[test]
    fn totp_computes_six_digit_code() {
        let compute = TotpCompute::from_secret(SHA1_B32, 30, 6).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(59);
        let code = compute.compute(now, 0).unwrap();
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(code.len(), 6);
    }

    #[test]
    fn drift_tolerance_computes_neighbouring_windows() {
        let compute = TotpCompute::from_secret(SHA1_B32, 30, 6).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(759);
        let center = compute.compute(now, 0).unwrap();
        let earlier = compute.compute(now, -1).unwrap();
        let later = compute.compute(now, 1).unwrap();
        // Centre code repeats only if drift lands in the same window.
        assert_eq!(compute.compute(now, 0).unwrap(), center);
        // Different windows must produce different codes.
        let windows = [earlier.clone(), center.clone(), later.clone()];
        let distinct: std::collections::HashSet<&String> = windows
            .iter()
            .collect::<std::collections::HashSet<&String>>();
        assert_eq!(distinct.len(), 3);
    }
}
