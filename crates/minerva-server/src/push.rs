//! Push notifications for reminders (Web Push): a browser or the installed app
//! subscribes (`push/subscribe`), and when a reminder is due its people get a
//! notification as well as the email.
//!
//! Messages are encrypted for each subscription as RFC 8291 describes
//! (`aes128gcm`: ECDH with the browser's key, HKDF, AES-128-GCM) and signed for
//! the push service with this server's VAPID key (RFC 8292), made once and kept
//! with the data. Only the push services of the major browsers are accepted as
//! subscription addresses, so the server never posts to other hosts. A
//! subscription the push service says is gone is dropped.
use super::*;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ring::{agreement, hkdf, rand::SecureRandom, signature};
use std::time::Duration;

/// A person's subscription, as the browser gave it.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct Subscription {
    pub(super) user_id: String,
    endpoint: String,
    /// The browser's public key (P-256, uncompressed) and auth secret.
    p256dh: String,
    auth: String,
    created_at: u64,
}
/// A notification waiting to go out.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct Message {
    user_id: String,
    title: String,
    body: String,
    url: String,
    next_try: u64,
    #[serde(default)]
    tries: u32,
}

/// Push services of the browsers people use; addresses elsewhere are refused.
const SERVICES: [&str; 6] = [
    "fcm.googleapis.com",
    "android.googleapis.com",
    "updates.push.services.mozilla.com",
    "push.services.mozilla.com",
    "web.push.apple.com",
    "notify.windows.com",
];
fn allowed_endpoint(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    url.scheme() == "https"
        && url.port().is_none()
        && url.host_str().is_some_and(|h| {
            SERVICES
                .iter()
                .any(|s| h == *s || h.ends_with(&format!(".{s}")))
        })
}

/// The server's VAPID key pair (PKCS#8, base64), made the first time.
pub(super) fn ensure_keys(d: &mut Data) {
    if !d.vapid.is_empty() {
        return;
    }
    let rng = ring::rand::SystemRandom::new();
    if let Ok(pkcs8) =
        signature::EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
    {
        d.vapid = STANDARD.encode(pkcs8.as_ref());
    }
}
fn key_pair(d: &Data) -> Option<signature::EcdsaKeyPair> {
    let pkcs8 = STANDARD.decode(&d.vapid).ok()?;
    signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &pkcs8,
        &ring::rand::SystemRandom::new(),
    )
    .ok()
}
/// The public key browsers subscribe with (`applicationServerKey`).
fn public_key(d: &Data) -> Option<String> {
    use signature::KeyPair;
    Some(URL_SAFE_NO_PAD.encode(key_pair(d)?.public_key().as_ref()))
}

/// `push`: `GET push` (the key, and whether this device's subscription is
/// known), `POST push/subscribe {endpoint, keys: {p256dh, auth}}`,
/// `POST push/unsubscribe {endpoint}`.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("GET", []) => Ok(json!({
            "key": public_key(d),
            "subscriptions": d.push_subscriptions.iter().filter(|s| s.user_id == u.id).count(),
        })),
        ("POST", ["subscribe"]) => {
            let endpoint = v["endpoint"].as_str().unwrap_or("").to_string();
            if endpoint.len() > 1000 || !allowed_endpoint(&endpoint) {
                return Err(bad("This push service is not supported"));
            }
            let p256dh = v["keys"]["p256dh"].as_str().unwrap_or("");
            let auth = v["keys"]["auth"].as_str().unwrap_or("");
            let fits = URL_SAFE_NO_PAD
                .decode(p256dh.trim_end_matches('='))
                .is_ok_and(|k| k.len() == 65 && k[0] == 4)
                && URL_SAFE_NO_PAD
                    .decode(auth.trim_end_matches('='))
                    .is_ok_and(|a| a.len() == 16);
            if !fits {
                return Err(bad("Invalid push subscription"));
            }
            d.push_subscriptions.retain(|s| s.endpoint != endpoint);
            if d.push_subscriptions
                .iter()
                .filter(|s| s.user_id == u.id)
                .count()
                >= 10
            {
                return Err(bad("Turn notifications off on another device first"));
            }
            d.push_subscriptions.push(Subscription {
                user_id: u.id.clone(),
                endpoint,
                p256dh: p256dh.trim_end_matches('=').into(),
                auth: auth.trim_end_matches('=').into(),
                created_at: now(),
            });
            Ok(json!({"ok": true}))
        }
        ("POST", ["unsubscribe"]) => {
            let endpoint = v["endpoint"].as_str().unwrap_or("");
            d.push_subscriptions
                .retain(|s| !(s.user_id == u.id && s.endpoint == endpoint));
            Ok(json!({"ok": true}))
        }
        _ => Err(missing()),
    }
}
/// Forgets a deleted account's subscriptions and messages.
pub(super) fn forget_user(d: &mut Data, user_id: &str) {
    d.push_subscriptions.retain(|s| s.user_id != user_id);
    d.push_outbox.retain(|m| m.user_id != user_id);
}

/// Queues a notification for `user_id`, if they have subscribed somewhere.
pub(super) fn queue(d: &mut Data, user_id: &str, title: &str, body: &str, url: &str) {
    if !d.push_subscriptions.iter().any(|s| s.user_id == user_id) || d.push_outbox.len() > 1000 {
        return;
    }
    d.push_outbox.push(Message {
        user_id: user_id.into(),
        title: title.chars().take(120).collect(),
        body: body.chars().take(300).collect(),
        url: url.into(),
        next_try: now(),
        tries: 0,
    });
    wake().notify_one();
}

// ---------------------------------------------------------------------------
// RFC 8291: the message, encrypted for one subscription.

struct Len(usize);
impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}
fn expand(prk: &hkdf::Prk, info: &[&[u8]], len: usize) -> Option<Vec<u8>> {
    let mut out = vec![0; len];
    prk.expand(info, Len(len)).ok()?.fill(&mut out).ok()?;
    Some(out)
}
/// The content key and nonce, from the ECDH secret, both public keys, the
/// subscription's auth secret and the message's salt.
fn content_keys(
    ecdh: &[u8],
    auth: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    salt: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let prk_key = hkdf::Salt::new(hkdf::HKDF_SHA256, auth).extract(ecdh);
    let ikm = expand(&prk_key, &[b"WebPush: info\0", ua_public, as_public], 32)?;
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&ikm);
    Some((
        expand(&prk, &[b"Content-Encoding: aes128gcm\0"], 16)?,
        expand(&prk, &[b"Content-Encoding: nonce\0"], 12)?,
    ))
}
/// The `aes128gcm` body: salt, record size, the server's key, the ciphertext.
fn seal(plain: &[u8], key: &[u8], nonce: &[u8], salt: &[u8], as_public: &[u8]) -> Option<Vec<u8>> {
    use ring::aead;
    let mut data = plain.to_vec();
    // The last (and only) record's delimiter.
    data.push(2);
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, key).ok()?);
    key.seal_in_place_append_tag(
        aead::Nonce::try_assume_unique_for_key(nonce).ok()?,
        aead::Aad::empty(),
        &mut data,
    )
    .ok()?;
    let mut out = salt.to_vec();
    out.extend(4096u32.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend(as_public);
    out.extend(data);
    Some(out)
}
fn encrypt(plain: &[u8], ua_public: &[u8], auth: &[u8]) -> Option<Vec<u8>> {
    let rng = ring::rand::SystemRandom::new();
    let private = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).ok()?;
    let as_public = private.compute_public_key().ok()?.as_ref().to_vec();
    let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, ua_public);
    let ecdh = agreement::agree_ephemeral(private, &peer, |secret| secret.to_vec()).ok()?;
    let mut salt = [0u8; 16];
    rng.fill(&mut salt).ok()?;
    let (key, nonce) = content_keys(&ecdh, auth, ua_public, &as_public, &salt)?;
    seal(plain, &key, &nonce, &salt, &as_public)
}
/// RFC 8292: the push service learns which server sends, signed.
fn vapid_header(d: &Data, endpoint: &str) -> Option<String> {
    let pair = key_pair(d)?;
    let url = url::Url::parse(endpoint).ok()?;
    let audience = url.origin().ascii_serialization();
    let subject = std::env::var("MINERVA_PUBLIC_URL")
        .ok()
        .filter(|u| u.starts_with("https://"))
        .unwrap_or_else(|| "mailto:admin@minerva.local".into());
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = URL_SAFE_NO_PAD
        .encode(json!({"aud": audience, "exp": now() + 12 * 3600, "sub": subject}).to_string());
    let signed = format!("{header}.{claims}");
    let signature = pair
        .sign(&ring::rand::SystemRandom::new(), signed.as_bytes())
        .ok()?;
    let token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()));
    Some(format!("vapid t={token}, k={}", public_key(d)?))
}

// ---------------------------------------------------------------------------
// The sender.

fn wake() -> &'static tokio::sync::Notify {
    static WAKE: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    WAKE.get_or_init(tokio::sync::Notify::new)
}
/// Starts the sender; without email, it also notices due reminders.
pub(crate) fn spawn(store: Shared) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        loop {
            tick(&store, &client).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = wake().notified() => {}
            }
        }
    });
}
/// For one waiting message (by index): each subscription's address, VAPID
/// header and encrypted body.
type Batch = Vec<(usize, Vec<(String, String, Vec<u8>)>)>;
async fn tick(store: &Shared, client: &reqwest::Client) {
    // What to send, prepared under the lock; sent without it.
    let batch: Batch = {
        let Ok(mut s) = store.lock() else { return };
        let mut d = s.data.clone();
        let mut changed = false;
        if !mail::enabled() {
            changed |= reminders::fire(&mut d);
        }
        let mut batch = vec![];
        for (i, m) in d
            .push_outbox
            .iter()
            .enumerate()
            .filter(|(_, m)| m.next_try <= now())
            .take(50)
        {
            let payload = json!({"title": m.title, "body": m.body, "url": m.url}).to_string();
            let sends = d
                .push_subscriptions
                .iter()
                .filter(|sub| sub.user_id == m.user_id)
                .filter_map(|sub| {
                    let ua = URL_SAFE_NO_PAD.decode(&sub.p256dh).ok()?;
                    let auth = URL_SAFE_NO_PAD.decode(&sub.auth).ok()?;
                    let body = encrypt(payload.as_bytes(), &ua, &auth)?;
                    Some((sub.endpoint.clone(), vapid_header(&d, &sub.endpoint)?, body))
                })
                .collect();
            batch.push((i, sends));
        }
        if changed && persist(&mut s, d).is_err() {
            eprintln!("Could not save due reminders");
        }
        batch
    };
    if batch.is_empty() {
        return;
    }
    let mut gone: Vec<String> = vec![];
    let mut sent: Vec<usize> = vec![];
    let mut failed: Vec<usize> = vec![];
    for (i, sends) in batch {
        let mut ok = true;
        for (endpoint, auth, body) in sends {
            let response = client
                .post(&endpoint)
                .header("authorization", auth)
                .header("content-encoding", "aes128gcm")
                .header("content-type", "application/octet-stream")
                .header("ttl", "86400")
                .header("urgency", "high")
                .body(body)
                .send()
                .await;
            match response.map(|r| r.status().as_u16()) {
                Ok(200..=299) => {}
                // The subscription ended (unsubscribed, or the app removed).
                Ok(404 | 410) => gone.push(endpoint),
                _ => ok = false,
            }
        }
        if ok {
            sent.push(i);
        } else {
            failed.push(i);
        }
    }
    let Ok(mut s) = store.lock() else { return };
    let mut d = s.data.clone();
    d.push_subscriptions
        .retain(|sub| !gone.contains(&sub.endpoint));
    let mut kept = vec![];
    for (i, mut m) in std::mem::take(&mut d.push_outbox).into_iter().enumerate() {
        if sent.contains(&i) {
            continue;
        }
        if failed.contains(&i) {
            m.tries += 1;
            m.next_try = now() + 60 * u64::from(m.tries.min(30));
            if m.tries > 5 {
                continue;
            }
        }
        kept.push(m);
    }
    d.push_outbox = kept;
    if persist(&mut s, d).is_err() {
        eprintln!("Could not save the notifications sent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b64(s: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(s).unwrap()
    }
    #[test]
    fn encryption_follows_rfc_8291() {
        // RFC 8291, Appendix A.
        let ua_public = b64("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let as_public = b64("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let salt = b64("DGv6ra1nlYgDCS1FRnbzlw");
        let ecdh = b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let (key, nonce) = content_keys(&ecdh, &auth, &ua_public, &as_public, &salt).unwrap();
        assert_eq!(URL_SAFE_NO_PAD.encode(&key), "oIhVW04MRdy2XN9CiKLxTg");
        assert_eq!(URL_SAFE_NO_PAD.encode(&nonce), "4h_95klXJ5E_qnoN");
        let body = seal(
            b"When I grow up, I want to be a watermelon",
            &key,
            &nonce,
            &salt,
            &as_public,
        )
        .unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }
    #[test]
    fn a_message_a_browser_can_open() {
        // The browser's side: its key pair, and the server's message for it.
        let rng = ring::rand::SystemRandom::new();
        let browser =
            agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).unwrap();
        let ua_public = browser.compute_public_key().unwrap().as_ref().to_vec();
        let auth = [7u8; 16];
        let body = encrypt(b"{\"title\":\"Standup\"}", &ua_public, &auth).unwrap();
        let (salt, rest) = body.split_at(16);
        assert_eq!(&rest[..4], &4096u32.to_be_bytes());
        let as_public = &rest[5..5 + usize::from(rest[4])];
        let mut cipher = rest[5 + as_public.len()..].to_vec();
        let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, as_public);
        let ecdh = agreement::agree_ephemeral(browser, &peer, |s| s.to_vec()).unwrap();
        let (key, nonce) = content_keys(&ecdh, &auth, &ua_public, as_public, salt).unwrap();
        let key = ring::aead::LessSafeKey::new(
            ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &key).unwrap(),
        );
        let plain = key
            .open_in_place(
                ring::aead::Nonce::try_assume_unique_for_key(&nonce).unwrap(),
                ring::aead::Aad::empty(),
                &mut cipher,
            )
            .unwrap();
        assert_eq!(plain, b"{\"title\":\"Standup\"}\x02");
    }
    #[test]
    fn vapid_tokens_are_signed_and_only_known_services_are_used() {
        let mut d = Data::default();
        ensure_keys(&mut d);
        let header = vapid_header(&d, "https://fcm.googleapis.com/fcm/send/abc").unwrap();
        let token = header
            .strip_prefix("vapid t=")
            .unwrap()
            .split(", k=")
            .next()
            .unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let claims: Value = serde_json::from_slice(&b64(parts[1])).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        let public = b64(&public_key(&d).unwrap());
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &b64(parts[2]),
            )
            .unwrap();
        for ok in [
            "https://fcm.googleapis.com/fcm/send/x",
            "https://updates.push.services.mozilla.com/wpush/v2/x",
            "https://web.push.apple.com/abc",
            "https://db5p.notify.windows.com/w/?token=x",
        ] {
            assert!(allowed_endpoint(ok), "{ok}");
        }
        for bad in [
            "http://fcm.googleapis.com/x",
            "https://evil.example/fcm.googleapis.com",
            "https://fcm.googleapis.com.evil.example/x",
            "https://127.0.0.1/x",
            "https://fcm.googleapis.com:8443/x",
        ] {
            assert!(!allowed_endpoint(bad), "{bad}");
        }
    }
}
