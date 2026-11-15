//! Single sign-on with an OpenID Connect provider (Keycloak, Authentik, Google
//! Workspace, Entra ID, ...): the authorization code flow with PKCE, one
//! provider per server, configured with `MINERVA_OIDC_*`.
//!
//! - **Which account.** An account is bound to the provider's `(issuer, sub)`
//!   pair, which never changes. The first sign-in finds it by email, only when the
//!   provider says the email is verified. Administrator accounts are never linked
//!   by email: they link from Settings while signed in with their password, so
//!   whoever controls the provider cannot take over the server.
//! - **New accounts** are off unless `MINERVA_OIDC_CREATE_DOMAINS` lists the
//!   email domains allowed to create one; they become ordinary members.
//! - **Passwords** keep working. `MINERVA_OIDC_REQUIRED=1` refuses them for
//!   everyone but administrators, who keep a way in when the provider is down.
//! - **Second factor.** An SSO sign-in does not ask for Minerva's own 2FA; the
//!   provider is responsible for it.
//!
//! The browser comes back to `/auth/sso/callback`. A web sign-in gets the usual
//! session cookie. The desktop app listens on this computer (RFC 8252): it gets
//! a one-time code at `http://127.0.0.1:{port}/sso` and exchanges it, with the
//! PKCE-style verifier only it knows, at `POST /api/v1/login/sso`.
use super::*;
use axum::extract::Query;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use std::collections::HashMap as Map;

/// How long the provider may take to send the person back.
const PENDING_SECONDS: u64 = 600;
/// How long the desktop app has to exchange its code.
const HANDOFF_SECONDS: u64 = 60;
/// How long the provider's discovery document and keys are kept.
const DISCOVERY_SECONDS: u64 = 3600;
/// Sign-ins in progress kept at once. Starting one needs no account, so a full
/// list drops its oldest entry rather than refusing everyone for ten minutes.
const MAX_PENDING: usize = 10_000;
const COOKIE: &str = "minerva_sso";

/// The settings, read on every request so they follow the environment.
pub(super) struct Settings {
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
    pub(super) name: String,
    create_domains: Vec<String>,
    pub(super) required: bool,
    public: String,
}
fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
/// SSO is on when the provider, the client ID and the server's public address
/// are all set.
pub(super) fn settings() -> Option<Settings> {
    let issuer = env("MINERVA_OIDC_ISSUER")?;
    let client_id = env("MINERVA_OIDC_CLIENT_ID")?;
    let public = env("MINERVA_PUBLIC_URL")?.trim_end_matches('/').to_string();
    Some(Settings {
        issuer,
        client_id,
        client_secret: env("MINERVA_OIDC_CLIENT_SECRET"),
        name: env("MINERVA_OIDC_NAME").unwrap_or_else(|| "single sign-on".into()),
        create_domains: env("MINERVA_OIDC_CREATE_DOMAINS")
            .unwrap_or_default()
            .split(',')
            .map(|d| d.trim().trim_start_matches('@').to_lowercase())
            .filter(|d| !d.is_empty())
            .collect(),
        required: env("MINERVA_OIDC_REQUIRED").as_deref() == Some("1"),
        public,
    })
}
/// What the sign-in page needs to know, for `/api/v1/health`.
pub(super) fn health() -> Value {
    settings().map_or(
        Value::Null,
        |s| json!({"name": s.name, "required": s.required}),
    )
}
/// Whether this account must sign in with SSO instead of its password.
pub(super) fn password_refused(user: &User) -> Option<String> {
    settings()
        .filter(|s| s.required && !user.admin)
        .map(|s| s.name)
}
/// The answer to a password sign-in that `password_refused`.
pub(super) fn password_error(name: &str) -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        i18n::fill(
            i18n::msg(
                "Sign in with {name}: this server does not accept a password for your account.",
            ),
            &[("name", name)],
        ),
    )
}

/// The provider identity an account is bound to.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct Link {
    issuer: String,
    subject: String,
    linked_at: u64,
}
/// A sign-in sent to the provider and not back yet (memory only).
#[derive(Clone)]
pub(super) struct Pending {
    nonce: String,
    verifier: String,
    intent: Intent,
    expires: u64,
}
#[derive(Clone)]
enum Intent {
    Web,
    Desktop {
        port: u16,
        challenge: String,
    },
    /// Linking the provider identity to an account already signed in.
    Link {
        user_id: String,
    },
}
/// A desktop sign-in waiting for the app to collect it (memory only).
#[derive(Clone)]
pub(super) struct Handoff {
    user_id: String,
    challenge: String,
    expires: u64,
}

fn off() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}
fn secure() -> &'static str {
    if std::env::var("MINERVA_SECURE_COOKIE").as_deref() == Ok("1") {
        "; Secure"
    } else {
        ""
    }
}
fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // The provider's endpoints must answer themselves, not redirect.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default()
    })
}
/// The provider's discovery document and keys, fetched again after an hour, or
/// at once with `fresh` (a token signed with a key we do not have yet).
async fn provider(s: &Settings, fresh: bool) -> Result<CoreProviderMetadata, String> {
    static CACHE: tokio::sync::Mutex<Option<(String, u64, CoreProviderMetadata)>> =
        tokio::sync::Mutex::const_new(None);
    let mut cache = CACHE.lock().await;
    if let Some((issuer, at, meta)) = cache.as_ref() {
        if !fresh && *issuer == s.issuer && at + DISCOVERY_SECONDS > now() {
            return Ok(meta.clone());
        }
    }
    let issuer = IssuerUrl::new(s.issuer.clone()).map_err(|e| format!("issuer: {e}"))?;
    let meta = CoreProviderMetadata::discover_async(issuer, http())
        .await
        .map_err(|e| format!("discovery: {}", chain(&e)))?;
    *cache = Some((s.issuer.clone(), now(), meta.clone()));
    Ok(meta)
}
/// An error and its causes, for the log.
fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    text
}
fn redirect_url(s: &Settings) -> Result<RedirectUrl, String> {
    RedirectUrl::new(format!("{}/auth/sso/callback", s.public)).map_err(|e| e.to_string())
}
/// A page explaining why the sign-in stopped, with a way back.
fn failed(lang: &str, message: &str) -> Response {
    oauth::page(
        lang,
        &i18n::tr(lang, "Sign-in did not work"),
        format!(
            "<p>{}</p><p><a href=\"/\">{}</a></p>",
            oauth::escape(&i18n::tr(lang, message)),
            oauth::escape(&i18n::tr(lang, "Back to Minerva"))
        ),
    )
}
fn found(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [
            ("location", location.to_string()),
            ("cache-control", "no-store".into()),
        ],
    )
        .into_response()
}
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .find_map(|p| p.trim().strip_prefix(name)?.strip_prefix('='))
}
/// A desktop app's PKCE challenge: base64url SHA-256, 43 characters.
fn valid_challenge(c: &str) -> bool {
    c.len() == 43
        && c.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `GET /auth/sso/start?client=web|desktop|link`: sends the browser to the
/// provider, remembering the sign-in in memory and in a short-lived cookie.
pub(super) async fn start(
    axum::extract::State(store): axum::extract::State<Shared>,
    headers: HeaderMap,
    Query(q): Query<Map<String, String>>,
) -> Response {
    let Some(cfg) = settings() else {
        return off();
    };
    let lang = i18n::for_request(&headers);
    let intent = match q.get("client").map(String::as_str) {
        Some("desktop") => {
            let port = q.get("port").and_then(|p| p.parse::<u16>().ok());
            let challenge = q.get("challenge").cloned().unwrap_or_default();
            match port {
                Some(port) if port >= 1024 && valid_challenge(&challenge) => {
                    Intent::Desktop { port, challenge }
                }
                _ => {
                    return failed(
                        &lang,
                        i18n::msg("The app sent an incomplete sign-in request."),
                    )
                }
            }
        }
        Some("link") => {
            let Ok(s) = store.lock() else {
                return internal().into_response();
            };
            // The session cookie is `SameSite=Strict`: another site cannot start this.
            match oauth::signed_in(&s.data, &headers) {
                Some(u) => Intent::Link { user_id: u.id },
                None => {
                    return failed(
                        &lang,
                        i18n::msg(
                            "Sign in to Minerva first, then link your account from Settings.",
                        ),
                    )
                }
            }
        }
        _ => Intent::Web,
    };
    let meta = match provider(&cfg, false).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("SSO: cannot reach the provider: {e}");
            return failed(
                &lang,
                i18n::msg("The sign-in provider cannot be reached. Try again later."),
            );
        }
    };
    let redirect = match redirect_url(&cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("SSO: MINERVA_PUBLIC_URL is not a valid address: {e}");
            return failed(
                &lang,
                i18n::msg("Single sign-on is not set up correctly on this server."),
            );
        }
    };
    let client = oidc_client(meta, &cfg, redirect);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(challenge)
        .url();
    {
        let Ok(mut s) = store.lock() else {
            return internal().into_response();
        };
        let at = now();
        s.data.sso_pending.retain(|_, p| p.expires > at);
        while s.data.sso_pending.len() >= MAX_PENDING {
            let oldest = s
                .data
                .sso_pending
                .iter()
                .min_by_key(|(_, p)| p.expires)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => s.data.sso_pending.remove(&k),
                None => break,
            };
        }
        s.data.sso_pending.insert(
            hash_token(state.secret()),
            Pending {
                nonce: nonce.secret().clone(),
                verifier: verifier.secret().clone(),
                intent,
                expires: at + PENDING_SECONDS,
            },
        );
    }
    // `Lax`, so the cookie comes back when the provider sends the browser here.
    let cookie = format!(
        "{COOKIE}={}; HttpOnly; SameSite=Lax; Path=/auth/sso; Max-Age={PENDING_SECONDS}{}",
        state.secret(),
        secure()
    );
    let mut response = found(url.as_str());
    if let Ok(value) = cookie.parse() {
        response.headers_mut().append("set-cookie", value);
    }
    response
}

/// Who the provider says signed in.
struct Identity {
    issuer: String,
    subject: String,
    email: Option<String>,
    email_verified: bool,
    name: Option<String>,
}

/// Exchanges the code and checks the ID token: signature, issuer, audience,
/// expiry, nonce, and the access token's hash when the token carries one. A
/// token signed with a key we do not know yet (the provider rotated its keys)
/// fetches the keys again, once.
async fn identify(cfg: &Settings, code: &str, pending: &Pending) -> Result<Identity, String> {
    let redirect = redirect_url(cfg)?;
    let mut meta = provider(cfg, false).await?;
    let tokens = oidc_client(meta.clone(), cfg, redirect.clone())
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|e| format!("token endpoint: {e}"))?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.verifier.clone()))
        .request_async(http())
        .await
        .map_err(|e| format!("token exchange: {}", chain(&e)))?;
    let id_token = tokens
        .id_token()
        .ok_or("the provider sent no ID token")?
        .clone();
    let nonce = Nonce::new(pending.nonce.clone());
    let mut retried = false;
    loop {
        let client = oidc_client(meta.clone(), cfg, redirect.clone());
        let verifier = client.id_token_verifier();
        let claims = match id_token.claims(&verifier, &nonce) {
            Ok(c) => c,
            Err(openidconnect::ClaimsVerificationError::SignatureVerification(
                openidconnect::SignatureVerificationError::NoMatchingKey,
            )) if !retried => {
                retried = true;
                drop(verifier);
                meta = provider(cfg, true).await?;
                continue;
            }
            Err(e) => return Err(format!("ID token: {}", chain(&e))),
        };
        if let Some(expected) = claims.access_token_hash() {
            let alg = id_token
                .signing_alg()
                .map_err(|e| format!("ID token: {e}"))?;
            let key = id_token
                .signing_key(&verifier)
                .map_err(|e| format!("ID token: {e}"))?;
            let actual = AccessTokenHash::from_token(tokens.access_token(), alg, key)
                .map_err(|e| format!("ID token: {e}"))?;
            if actual != *expected {
                return Err("ID token: the access token hash does not match".into());
            }
        }
        return Ok(Identity {
            issuer: claims.issuer().as_str().to_string(),
            subject: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().trim().to_lowercase()),
            email_verified: claims.email_verified() == Some(true),
            name: claims
                .name()
                .and_then(|n| n.get(None))
                .map(|n| n.as_str().trim().to_string())
                .filter(|n| !n.is_empty()),
        });
    }
}
fn oidc_client(
    meta: CoreProviderMetadata,
    cfg: &Settings,
    redirect: RedirectUrl,
) -> CoreClient<
    openidconnect::EndpointSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointMaybeSet,
    openidconnect::EndpointMaybeSet,
> {
    CoreClient::from_provider_metadata(
        meta,
        ClientId::new(cfg.client_id.clone()),
        cfg.client_secret.clone().map(ClientSecret::new),
    )
    .set_redirect_uri(redirect)
}

const NO_ACCOUNT: &str = i18n::msg(
    "There is no Minerva account for this sign-in. Ask your administrator to create one.",
);
const DISABLED: &str = i18n::msg("This Minerva account is disabled. Ask your administrator.");
const ADMIN: &str = i18n::msg(
    "Administrator accounts are not linked automatically. Sign in with your password, then link single sign-on from Settings.",
);
const TAKEN: &str = i18n::msg("This sign-in is already linked to another Minerva account.");
const BROKEN: &str = i18n::msg("Something went wrong on the server. Try again.");

/// The account a provider identity signs in to, linking or creating it as the
/// rules in this module's comment say.
fn resolve(
    d: &mut Data,
    cfg: &Settings,
    who: &Identity,
    intent: &Intent,
    lang: &str,
) -> Result<User, &'static str> {
    let bound = |a: &Account| {
        a.sso
            .as_ref()
            .is_some_and(|l| l.issuer == who.issuer && l.subject == who.subject)
    };
    let link = Link {
        issuer: who.issuer.clone(),
        subject: who.subject.clone(),
        linked_at: now(),
    };
    if let Intent::Link { user_id } = intent {
        if d.users.iter().any(|a| bound(a) && a.user.id != *user_id) {
            return Err(TAKEN);
        }
        let a = d
            .users
            .iter_mut()
            .find(|a| a.user.id == *user_id && !a.user.disabled)
            .ok_or(DISABLED)?;
        a.sso = Some(link);
        return Ok(a.user.clone());
    }
    if let Some(a) = d.users.iter().find(|a| bound(a)) {
        return if a.user.disabled {
            Err(DISABLED)
        } else {
            Ok(a.user.clone())
        };
    }
    // From here on the email decides, so it must be one the provider verified.
    let Some(email) = who
        .email
        .clone()
        .filter(|e| who.email_verified && e.contains('@'))
    else {
        return Err(NO_ACCOUNT);
    };
    if let Some(a) = d.users.iter_mut().find(|a| a.user.email == email) {
        if a.user.disabled {
            return Err(DISABLED);
        }
        // Bound to another identity of the provider: the email alone is not enough.
        if a.sso.is_some() {
            return Err(NO_ACCOUNT);
        }
        if a.user.admin {
            return Err(ADMIN);
        }
        a.sso = Some(link);
        return Ok(a.user.clone());
    }
    let domain = email.rsplit_once('@').map_or("", |(_, d)| d);
    if !cfg.create_domains.iter().any(|d| d == domain) {
        return Err(NO_ACCOUNT);
    }
    let name: String = who
        .name
        .clone()
        .unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string())
        .chars()
        .take(100)
        .collect();
    let user = User {
        token_workspace: None,
        id: id(),
        name,
        email,
        admin: false,
        can_create_workspace: true,
        disabled: false,
    };
    // A password nobody knows: this account signs in with SSO.
    let password_hash = password_hash(&oauth::secret()).map_err(|_| BROKEN)?;
    d.users.push(Account {
        user: user.clone(),
        password_hash,
        two_factor: Default::default(),
        must_change_password: false,
        default_workspace: None,
        dashboard: None,
        sso: Some(link),
    });
    let lang = if std::env::var("MINERVA_LANGUAGE").is_ok() {
        i18n::default_language()
    } else {
        lang.to_string()
    };
    create_personal(d, &user.id, &lang);
    Ok(user)
}

/// `GET /auth/sso/callback`: the provider sends the browser back here.
pub(super) async fn callback(
    axum::extract::State(store): axum::extract::State<Shared>,
    headers: HeaderMap,
    Query(q): Query<Map<String, String>>,
) -> Response {
    let Some(cfg) = settings() else {
        return off();
    };
    let lang = i18n::for_request(&headers);
    let state = q.get("state").cloned().unwrap_or_default();
    // Used once, whatever happens next.
    let pending = {
        let Ok(mut s) = store.lock() else {
            return internal().into_response();
        };
        let at = now();
        s.data.sso_pending.retain(|_, p| p.expires > at);
        s.data.sso_pending.remove(&hash_token(&state))
    };
    // Only the browser that started the sign-in may finish it.
    let pending = pending
        .filter(|_| !state.is_empty() && cookie_value(&headers, COOKIE) == Some(state.as_str()));
    let mut response = match pending {
        None => failed(
            &lang,
            i18n::msg("This sign-in has expired or was started in another browser. Start again from the sign-in page."),
        ),
        Some(pending) => {
            let desktop = match &pending.intent {
                Intent::Desktop { port, .. } => Some(*port),
                _ => None,
            };
            match finish(&store, &cfg, &lang, &q, pending).await {
                Ok(response) => response,
                // The app waits for an answer: it hears why, and shows it.
                Err(message) => match desktop {
                    Some(port) => found(&format!(
                        "http://127.0.0.1:{port}/sso?error={}",
                        url::form_urlencoded::byte_serialize(message.as_bytes()).collect::<String>()
                    )),
                    None => failed(&lang, message),
                },
            }
        }
    };
    let clear = format!(
        "{COOKIE}=; HttpOnly; SameSite=Lax; Path=/auth/sso; Max-Age=0{}",
        secure()
    );
    if let Ok(value) = clear.parse() {
        response.headers_mut().append("set-cookie", value);
    }
    response
}
/// The rest of a sign-in whose state checked out; an error is the message to
/// show, in English (the browser's page and the app translate it).
async fn finish(
    store: &Shared,
    cfg: &Settings,
    lang: &str,
    q: &Map<String, String>,
    pending: Pending,
) -> Result<Response, &'static str> {
    if let Some(error) = q.get("error") {
        eprintln!("SSO: the provider ended the sign-in: {error}");
        return Err(i18n::msg(
            "The sign-in was cancelled or refused by the provider.",
        ));
    }
    let code = q.get("code").filter(|c| !c.is_empty()).ok_or(i18n::msg(
        "The provider sent an incomplete answer. Try again.",
    ))?;
    let who = identify(cfg, code, &pending).await.map_err(|e| {
        eprintln!("SSO: sign-in not verified: {e}");
        i18n::msg("The sign-in could not be verified. Try again.")
    })?;
    let mut s = store.lock().map_err(|_| BROKEN)?;
    let mut d = s.data.clone();
    let user = resolve(&mut d, cfg, &who, &pending.intent, lang).inspect_err(|message| {
        eprintln!(
            "SSO: no sign-in for subject {} of {}: {message}",
            who.subject, who.issuer
        );
    })?;
    Ok(match pending.intent {
        Intent::Web => {
            let (_, cookie) = start_session(&mut d, user, secure());
            persist(&mut s, d).map_err(|_| BROKEN)?;
            let mut response = moving_on(lang, &i18n::tr(lang, "Signed in"), "/");
            if let Some(value) = cookie.and_then(|c| c.parse().ok()) {
                response.headers_mut().append("set-cookie", value);
            }
            response
        }
        Intent::Desktop { port, challenge } => {
            let code = oauth::secret();
            let at = now();
            d.sso_handoffs.retain(|_, h| h.expires > at);
            d.sso_handoffs.insert(
                hash_token(&code),
                Handoff {
                    user_id: user.id,
                    challenge,
                    expires: at + HANDOFF_SECONDS,
                },
            );
            persist(&mut s, d).map_err(|_| BROKEN)?;
            found(&format!("http://127.0.0.1:{port}/sso?code={code}"))
        }
        Intent::Link { .. } => {
            persist(&mut s, d).map_err(|_| BROKEN)?;
            moving_on(lang, &i18n::tr(lang, "Single sign-on is linked"), "/")
        }
    })
}
/// A page that moves on to `to` by itself. Not a redirect: a redirect chain
/// that began at the provider is cross-site, so the browser would leave the
/// `SameSite=Strict` session cookie out of the next request.
fn moving_on(lang: &str, title: &str, to: &str) -> Response {
    let to = oauth::escape(to);
    let html = format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={to}\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title></head><body style=\"font-family:system-ui,sans-serif;max-width:520px;margin:48px auto;padding:0 16px\"><p><a href=\"{to}\">{continue_}</a></p></body></html>",
        title = oauth::escape(title),
        continue_ = oauth::escape(&i18n::tr(lang, "Continue to Minerva")),
    );
    (
        StatusCode::OK,
        [
            ("content-type", "text/html; charset=utf-8"),
            ("x-frame-options", "DENY"),
            (
                "content-security-policy",
                "default-src 'none'; frame-ancestors 'none'",
            ),
            ("cache-control", "no-store"),
        ],
        html,
    )
        .into_response()
}

/// `POST /api/v1/login/sso`: the desktop app exchanges its one-time code and
/// the verifier behind its challenge for the signed-in person. The code is used
/// up by the first attempt, right or wrong.
pub(super) fn redeem(d: &mut Data, v: &Value) -> Result<User, ApiError> {
    use base64::Engine;
    let expired = || {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "This sign-in has expired. Start again.".into(),
        )
    };
    let code = v["code"].as_str().unwrap_or("");
    let verifier = v["verifier"].as_str().unwrap_or("");
    let at = now();
    d.sso_handoffs.retain(|_, h| h.expires > at);
    let handoff = d
        .sso_handoffs
        .remove(&hash_token(code))
        .ok_or_else(expired)?;
    let actual = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    if !(43..=128).contains(&verifier.len()) || actual != handoff.challenge {
        return Err(expired());
    }
    d.users
        .iter()
        .find(|a| a.user.id == handoff.user_id && !a.user.disabled)
        .map(|a| a.user.clone())
        .ok_or_else(expired)
}
/// Forgets an account's link, so its next SSO sign-in matches by email again.
pub(super) fn unlink(account: &mut Account) {
    account.sso = None;
}
pub(super) fn linked(account: &Account) -> bool {
    account.sso.is_some()
}
