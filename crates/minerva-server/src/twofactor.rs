//! Two-factor authentication with authenticator apps (TOTP, RFC 6238: HMAC-SHA1,
//! 6 digits, 30-second steps) and single-use recovery codes.
//!
//! Sign-in with 2FA is two steps. A correct password returns a short-lived
//! challenge instead of a session, and `/login/2fa` exchanges that challenge plus a
//! code for a session. The password is never sent twice.
use super::*;
use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
use hmac::{Hmac, Mac};

const STEP: u64 = 30;
const CHALLENGE_SECONDS: u64 = 300;
const CHALLENGE_ATTEMPTS: u32 = 5;
const PENDING_SECONDS: u64 = 900;
const RECOVERY_CODES: usize = 10;

#[derive(Clone, Serialize, Deserialize, Default)]
pub(super) struct TwoFactor {
    /// Base32 secret once enabled.
    #[serde(default)]
    secret: Option<String>,
    /// Secret being set up and when it was generated; becomes `secret` once confirmed.
    #[serde(default)]
    pending: Option<(String, u64)>,
    /// SHA-256 hashes of unused recovery codes.
    #[serde(default)]
    recovery: Vec<String>,
    /// Last accepted time step, so a code cannot be used twice.
    #[serde(default)]
    last_step: u64,
    #[serde(default)]
    enabled_at: u64,
}
impl TwoFactor {
    pub(super) fn enabled(&self) -> bool {
        self.secret.is_some()
    }
}
#[derive(Clone)]
pub(super) struct Challenge {
    user_id: String,
    expires: u64,
    attempts: u32,
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            out.push(ALPHABET[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}
fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = vec![];
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let value = ALPHABET
            .iter()
            .position(|&a| a as char == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            out.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}
/// RFC 4226 HOTP value (6 digits) for a counter.
fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac =
        <Hmac<sha1::Sha1> as Mac>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 1_000_000
}
/// The time step a code matches (allowing one step of clock drift either way),
/// rejecting steps at or before `last_step`.
fn verify(secret: &str, code: &str, last_step: u64, at: u64) -> Option<u64> {
    let code: u32 = code
        .trim()
        .replace(' ', "")
        .parse()
        .ok()
        .filter(|_| code.trim().replace(' ', "").len() == 6)?;
    let secret = base32_decode(secret)?;
    let step = at / STEP;
    [step.saturating_sub(1), step, step + 1]
        .into_iter()
        .find(|&s| s > last_step && hotp(&secret, s) == code)
}
fn random(bytes: usize) -> Vec<u8> {
    let mut out = vec![0u8; bytes];
    OsRng.fill_bytes(&mut out);
    out
}
/// Ten codes like `k7q2-m9xd`; only their hashes are stored.
fn new_recovery_codes() -> (Vec<String>, Vec<String>) {
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| {
            let raw = base32_encode(&random(5)).to_lowercase();
            format!("{}-{}", &raw[..4], &raw[4..8])
        })
        .collect();
    let hashes = codes
        .iter()
        .map(|c| hash_token(&normalize_recovery(c)))
        .collect();
    (codes, hashes)
}
fn normalize_recovery(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}
/// Accepts an authenticator code or consumes a recovery code.
fn check(tf: &mut TwoFactor, code: &str) -> bool {
    let Some(secret) = tf.secret.clone() else {
        return false;
    };
    if let Some(step) = verify(&secret, code, tf.last_step, now()) {
        tf.last_step = step;
        return true;
    }
    let hash = hash_token(&normalize_recovery(code));
    if let Some(index) = tf.recovery.iter().position(|h| *h == hash) {
        tf.recovery.remove(index);
        return true;
    }
    false
}
fn issuer() -> String {
    std::env::var("MINERVA_TOTP_ISSUER").unwrap_or_else(|_| "Minerva".into())
}
fn account_mut<'a>(d: &'a mut Data, user_id: &str) -> Result<&'a mut Account, ApiError> {
    d.users
        .iter_mut()
        .find(|a| a.user.id == user_id)
        .ok_or_else(internal)
}

/// After a correct password: a challenge when 2FA is enabled, otherwise `None`.
pub(super) fn challenge(d: &mut Data, user_id: &str) -> Option<Value> {
    let account = d.users.iter().find(|a| a.user.id == user_id)?;
    if !account.two_factor.enabled() {
        return None;
    }
    let token = format!("{}{}", id(), id());
    d.login_challenges.retain(|_, c| c.expires > now());
    d.login_challenges.insert(
        hash_token(&token),
        Challenge {
            user_id: user_id.to_string(),
            expires: now() + CHALLENGE_SECONDS,
            attempts: 0,
        },
    );
    Some(json!({"two_factor_required": true, "challenge": token}))
}

/// `/login/2fa`: exchanges a challenge and a code for the signed-in user.
pub(super) fn complete(d: &mut Data, v: &Value) -> Result<User, ApiError> {
    let expired = || {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "Your sign-in expired. Enter your password again.".into(),
        )
    };
    let key = hash_token(v["challenge"].as_str().unwrap_or(""));
    let challenge = d
        .login_challenges
        .get(&key)
        .cloned()
        .filter(|c| c.expires > now())
        .ok_or_else(expired)?;
    let code = v["code"].as_str().unwrap_or("").to_string();
    let account = account_mut(d, &challenge.user_id)?;
    if account.user.disabled {
        d.login_challenges.remove(&key);
        return Err(expired());
    }
    if check(&mut account.two_factor, &code) {
        let user = account.user.clone();
        d.login_challenges.remove(&key);
        return Ok(user);
    }
    let attempts = challenge.attempts + 1;
    if attempts >= CHALLENGE_ATTEMPTS {
        d.login_challenges.remove(&key);
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Too many incorrect codes. Enter your password again.".into(),
        ));
    }
    d.login_challenges.get_mut(&key).unwrap().attempts = attempts;
    Err(ApiError(
        StatusCode::UNAUTHORIZED,
        "That code is not valid. Try the current code from your app.".into(),
    ))
}

/// `/account/2fa/*` for the signed-in user.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
    current_session: Option<String>,
) -> Result<Option<Value>, ApiError> {
    let result = match (method, parts) {
        ("GET", ["account", "2fa"]) => {
            let tf = &account_mut(d, &u.id)?.two_factor;
            json!({"enabled": tf.enabled(), "enabled_at": tf.enabled_at, "recovery_remaining": tf.recovery.len()})
        }
        ("POST", ["account", "2fa", "setup"]) => {
            let account = account_mut(d, &u.id)?;
            if account.two_factor.enabled() {
                return Err(bad("Two-factor authentication is already on"));
            }
            let secret = base32_encode(&random(20));
            account.two_factor.pending = Some((secret.clone(), now()));
            let label = format!("{}:{}", issuer(), u.email);
            let uri = format!(
                "otpauth://totp/{}?secret={secret}&issuer={}&algorithm=SHA1&digits=6&period=30",
                url::form_urlencoded::byte_serialize(label.as_bytes())
                    .collect::<String>()
                    .replace('+', "%20"),
                url::form_urlencoded::byte_serialize(issuer().as_bytes())
                    .collect::<String>()
                    .replace('+', "%20"),
            );
            let qr = qrcode::QrCode::new(uri.as_bytes())
                .map_err(|_| internal())?
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(200, 200)
                .dark_color(qrcode::render::svg::Color("#111827"))
                .light_color(qrcode::render::svg::Color("#ffffff"))
                .build();
            let grouped = secret
                .as_bytes()
                .chunks(4)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect::<Vec<_>>()
                .join(" ");
            json!({"secret": grouped, "uri": uri, "qr_svg": qr})
        }
        ("POST", ["account", "2fa", "enable"]) => {
            let account = account_mut(d, &u.id)?;
            let (secret, created) = account
                .two_factor
                .pending
                .clone()
                .filter(|(_, created)| created + PENDING_SECONDS > now())
                .ok_or_else(|| bad("Setup expired. Start again to get a new QR code."))?;
            let step = verify(&secret, v["code"].as_str().unwrap_or(""), 0, now())
                .ok_or_else(|| bad("That code does not match. Check the time on your phone and try the current code."))?;
            let _ = created;
            let (codes, hashes) = new_recovery_codes();
            account.two_factor = TwoFactor {
                secret: Some(secret),
                pending: None,
                recovery: hashes,
                last_step: step,
                enabled_at: now(),
            };
            // Other sessions were signed in without the second factor.
            let keep = current_session.map(|t| hash_token(&t));
            d.sessions
                .retain(|hash, s| s.user_id != u.id || Some(hash) == keep.as_ref());
            json!({"enabled": true, "recovery_codes": codes})
        }
        ("POST", ["account", "2fa", "disable"]) => {
            let password = v["password"].as_str().unwrap_or("");
            let account = account_mut(d, &u.id)?;
            let valid_password = PasswordHash::new(&account.password_hash).is_ok_and(|hash| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &hash)
                    .is_ok()
            });
            if !valid_password {
                return Err(ApiError(
                    StatusCode::UNAUTHORIZED,
                    "Your password is not correct".into(),
                ));
            }
            if !check(&mut account.two_factor, v["code"].as_str().unwrap_or("")) {
                return Err(ApiError(
                    StatusCode::UNAUTHORIZED,
                    "That code is not valid".into(),
                ));
            }
            account.two_factor = TwoFactor::default();
            json!({"enabled": false})
        }
        ("POST", ["account", "2fa", "recovery"]) => {
            let account = account_mut(d, &u.id)?;
            let secret = account
                .two_factor
                .secret
                .clone()
                .ok_or_else(|| bad("Two-factor authentication is off"))?;
            // Recovery codes cannot mint new recovery codes; require the app.
            let step = verify(
                &secret,
                v["code"].as_str().unwrap_or(""),
                account.two_factor.last_step,
                now(),
            )
            .ok_or_else(|| {
                ApiError(
                    StatusCode::UNAUTHORIZED,
                    "Enter the current code from your authenticator app".into(),
                )
            })?;
            let (codes, hashes) = new_recovery_codes();
            account.two_factor.last_step = step;
            account.two_factor.recovery = hashes;
            json!({"recovery_codes": codes})
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

/// Cancels pending second-factor sign-ins for a user (account changes).
pub(super) fn forget(d: &mut Data, user_id: &str) {
    d.login_challenges.retain(|_, c| c.user_id != user_id);
}
/// Administrators can turn 2FA off for someone who lost their device.
pub(super) fn reset(account: &mut Account) {
    account.two_factor = TwoFactor::default();
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// The current code for a Base32 secret (also used by integration tests via the API).
    pub(crate) fn code(secret: &str, at: u64) -> String {
        format!("{:06}", hotp(&base32_decode(secret).unwrap(), at / STEP))
    }
    #[test]
    fn rfc_6238_vectors_and_replay() {
        // RFC 6238 appendix B (SHA-1), truncated to 6 digits.
        let secret = base32_encode(b"12345678901234567890");
        assert_eq!(secret, "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        for (time, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code(&secret, time), expected);
        }
        assert_eq!(base32_decode(&secret).unwrap(), b"12345678901234567890");
        let at = 1_111_111_109;
        let step = verify(&secret, "081804", 0, at).unwrap();
        assert!(
            verify(&secret, "081804", step, at).is_none(),
            "a code cannot be reused"
        );
        assert!(
            verify(&secret, &code(&secret, at + 30), 0, at).is_some(),
            "one step of drift is accepted"
        );
        assert!(verify(&secret, &code(&secret, at + 90), 0, at).is_none());
        assert!(verify(&secret, "12345", 0, at).is_none());
        let (codes, hashes) = new_recovery_codes();
        assert_eq!(codes.len(), 10);
        assert_eq!(
            hashes[0],
            hash_token(&normalize_recovery(&codes[0].to_uppercase()))
        );
    }
}
