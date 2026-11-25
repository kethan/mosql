//! The engine every route talks to: configuration, sessions and tokens,
//! verification tokens, and the two outbound seams (HTTP and mail).
//!
//! The route modules are thin; this is where the invariants live. An access
//! token is an HS256 JWT carrying identity only (`sub`, `sid`, `email`,
//! `role`, `anon`, `imp`), good for fifteen minutes, verified with zero
//! database reads. The refresh token is opaque: 32 random bytes, stored on
//! the session row as a SHA-256 hex digest, rotated on every refresh so a
//! stolen copy stops working at the first use. Nothing here sets a cookie.

use std::collections::BTreeMap;
use std::sync::Arc;

use mosql::Value;

use crate::crypto::{self, Claims, Identity};
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::models::*;
use crate::oauth_presets::OAuthRegistry;
use crate::object;
use crate::store::{AuthStore, AuthStoreExt, WhereClause};

// The outbound HTTP client lives in the platform crate, so the platform's
// own adapters and this engine share one transport.
//
// The engine talks to the asynchronous face of it. A Worker cannot block on
// a socket, so its transport is a promise; the blocking transports answer
// through the same interface.
pub use nanobase::httpclient::{split_url, AsyncHttpClient, HttpRequest, HttpResponse};
#[cfg(feature = "native")]
pub use nanobase::httpclient::{CurlHttpClient, HttpClient, PlainHttpClient, SyncHttpClient};

/// Where the whole API hangs off the host.
pub const PATH_PREFIX: &str = "/api/auth";

/// The header the bearer client reads the access token from.
pub const TOKEN_HEADER: &str = "set-auth-token";

// ------------------------------------------------------------- outbound -----

/// Delivers a verification or OTP mail.
pub trait Mailer: Send + Sync {
    fn send(&self, to: &str, subject: &str, text: &str, html: Option<&str>) -> AuthResult<()>;
}

/// Writes mail to stderr, for development and tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsoleMailer;

impl Mailer for ConsoleMailer {
    fn send(&self, to: &str, subject: &str, text: &str, _html: Option<&str>) -> AuthResult<()> {
        eprintln!("[mail] to={to} subject={subject:?}\n{text}\n");
        Ok(())
    }
}

/// Records mail in memory, for tests that need to read the link back.
#[derive(Debug, Default)]
pub struct MemoryMailer {
    pub sent: std::sync::Mutex<Vec<(String, String, String)>>,
}

impl Mailer for MemoryMailer {
    fn send(&self, to: &str, subject: &str, text: &str, _html: Option<&str>) -> AuthResult<()> {
        self.sent.lock().expect("mailer").push((
            to.to_string(),
            subject.to_string(),
            text.to_string(),
        ));
        Ok(())
    }
}

// ------------------------------------------------------------- the engine ---

/// Everything the engine needs to know about its deployment.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthConfig {
    /// The public origin of this server, e.g. `https://app.example.com`.
    /// OAuth callbacks and emailed links are built from it.
    pub base_url: String,
    /// The HS256 key. At least 32 bytes of randomness in production.
    pub secret: Vec<u8>,
    /// Shown in mails and as the TOTP issuer.
    pub issuer: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub two_factor_pending_ttl_secs: i64,
    pub email_verification_ttl_secs: i64,
    pub password_reset_ttl_secs: i64,
    pub otp_ttl_secs: i64,
    pub otp_max_attempts: u32,
    pub magic_link_ttl_secs: i64,
    pub oauth_state_ttl_secs: i64,
    /// Refuse password sign-in until the address is verified.
    pub require_email_verification: bool,
    pub allow_anonymous: bool,
    /// The cost of a new password hash: scrypt's `N`.
    ///
    /// The stored hash does not carry it — Better Auth's format is just
    /// `<salt>:<key>` — so this has to be the same on both sides of the
    /// wire, and changing it makes every stored password unverifiable.
    /// [`crypto::SCRYPT_N`] is what the JavaScript stack uses.
    pub password_scrypt_n: u32,
    /// Origins allowed for CORS and as redirect targets, besides this
    /// server's own.
    pub allowed_origins: Vec<String>,
}

impl AuthConfig {
    pub fn new(base_url: impl Into<String>, secret: impl Into<Vec<u8>>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let issuer = base_url
            .split("://")
            .nth(1)
            .unwrap_or("better-auth")
            .split(['/', ':'])
            .next()
            .unwrap_or("better-auth")
            .to_string();
        AuthConfig {
            base_url,
            secret: secret.into(),
            issuer,
            access_ttl_secs: 15 * 60,
            refresh_ttl_secs: 30 * 24 * 60 * 60,
            two_factor_pending_ttl_secs: 5 * 60,
            email_verification_ttl_secs: 24 * 60 * 60,
            password_reset_ttl_secs: 15 * 60,
            otp_ttl_secs: 5 * 60,
            otp_max_attempts: 5,
            magic_link_ttl_secs: 5 * 60,
            oauth_state_ttl_secs: 10 * 60,
            require_email_verification: false,
            allow_anonymous: true,
            password_scrypt_n: crypto::SCRYPT_N,
            allowed_origins: Vec::new(),
        }
    }

    /// `BETTER_AUTH_SECRET` (or `SECRET_KEY`) and `BETTER_AUTH_URL` (or
    /// `BASE_URL`), with the rest optional.
    pub fn from_env() -> AuthResult<AuthConfig> {
        Self::from_env_with(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
    }

    pub fn from_env_with(lookup: impl Fn(&str) -> Option<String>) -> AuthResult<AuthConfig> {
        let secret = lookup("BETTER_AUTH_SECRET")
            .or_else(|| lookup("SECRET_KEY"))
            .ok_or_else(|| AuthError::internal("BETTER_AUTH_SECRET is not set"))?;
        if secret.len() < 32 {
            return Err(AuthError::internal(
                "BETTER_AUTH_SECRET must be at least 32 characters",
            ));
        }
        let base_url = lookup("BETTER_AUTH_URL")
            .or_else(|| lookup("BASE_URL"))
            .ok_or_else(|| AuthError::internal("BETTER_AUTH_URL is not set"))?;
        let mut config = AuthConfig::new(base_url, secret.into_bytes());
        if let Some(issuer) = lookup("BETTER_AUTH_ISSUER") {
            config.issuer = issuer;
        }
        config.require_email_verification = flag(lookup("BETTER_AUTH_REQUIRE_EMAIL_VERIFICATION"));
        config.allow_anonymous = !matches!(
            lookup("BETTER_AUTH_DISABLE_ANONYMOUS").as_deref(),
            Some("1" | "true")
        );
        if let Some(secs) = lookup("ACCESS_TTL_SECS").and_then(|v| v.parse().ok()) {
            config.access_ttl_secs = secs;
        }
        if let Some(secs) = lookup("REFRESH_TTL_SECS").and_then(|v| v.parse().ok()) {
            config.refresh_ttl_secs = secs;
        }
        if let Some(cost) = lookup("BETTER_AUTH_SCRYPT_N").and_then(|v| v.parse().ok()) {
            config.password_scrypt_n = cost;
        }
        if let Some(origins) = lookup("BETTER_AUTH_TRUSTED_ORIGINS") {
            config.allowed_origins = origins
                .split(',')
                .map(|o| o.trim().trim_end_matches('/').to_string())
                .filter(|o| !o.is_empty())
                .collect();
        }
        Ok(config)
    }
}

fn flag(value: Option<String>) -> bool {
    matches!(value.as_deref(), Some("1" | "true" | "yes"))
}

/// The engine. Cheap to clone behind an `Arc`; the route table takes
/// `&Auth`.
pub struct Auth {
    pub store: Arc<dyn AuthStore>,
    pub config: AuthConfig,
    pub oauth: OAuthRegistry,
    pub http: Option<Arc<dyn AsyncHttpClient>>,
    pub mailer: Option<Arc<dyn Mailer>>,
    /// What each provider's OpenID configuration said, read once.
    discovery: std::sync::Mutex<std::collections::HashMap<String, crate::oidc::Discovery>>,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("config", &self.config)
            .field("providers", &self.oauth.ids())
            .field("http", &self.http.is_some())
            .field("mailer", &self.mailer.is_some())
            .finish()
    }
}

impl Auth {
    pub fn new(store: Arc<dyn AuthStore>, config: AuthConfig) -> Self {
        Auth {
            store,
            config,
            oauth: OAuthRegistry::new(),
            http: None,
            mailer: None,
            discovery: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn with_oauth(mut self, registry: OAuthRegistry) -> Self {
        self.oauth = registry;
        self
    }

    pub fn with_http(mut self, http: Arc<dyn AsyncHttpClient>) -> Self {
        self.http = Some(http);
        self
    }

    pub fn with_http_opt(mut self, http: Option<Arc<dyn AsyncHttpClient>>) -> Self {
        self.http = http;
        self
    }

    pub fn with_mailer_opt(mut self, mailer: Option<Arc<dyn Mailer>>) -> Self {
        self.mailer = mailer;
        self
    }

    pub fn with_mailer(mut self, mailer: Arc<dyn Mailer>) -> Self {
        self.mailer = Some(mailer);
        self
    }

    pub fn now(&self) -> i64 {
        now_secs()
    }

    /// The outbound HTTP client, when there is one.
    ///
    /// Panics when there is none: every caller that reaches this has
    /// already checked.
    pub fn http_client(&self) -> &dyn AsyncHttpClient {
        self.http.as_deref().expect("no HttpClient configured")
    }

    /// A provider's endpoints, read from its OpenID configuration.
    ///
    /// The document is read once and remembered: it changes rarely, and
    /// every sign-in would otherwise pay for it again.
    pub async fn resolve_provider(
        &self,
        config: &crate::oauth_presets::ProviderConfig,
    ) -> AuthResult<crate::oauth_presets::ProviderConfig> {
        let Some(url) = config.discovery_url.clone() else {
            return Ok(config.clone());
        };
        if let Ok(cache) = self.discovery.lock() {
            if let Some(found) = cache.get(&url) {
                return crate::oidc::apply(config, found);
            }
        }
        let discovery =
            crate::oidc::Discovery::fetch(&url, &config.discovery_headers, self.http_client())
                .await?;
        if let Ok(mut cache) = self.discovery.lock() {
            cache.insert(url, discovery.clone());
        }
        crate::oidc::apply(config, &discovery)
    }

    /// `POST` something with the configured client.
    pub async fn send(&self, request: &HttpRequest) -> AuthResult<HttpResponse> {
        self.http
            .as_ref()
            .ok_or_else(|| AuthError::internal("no HttpClient configured; set one to use OAuth"))?
            .request(request)
            .await
    }

    pub fn send_mail(
        &self,
        to: &str,
        subject: &str,
        text: &str,
        html: Option<&str>,
    ) -> AuthResult<()> {
        match &self.mailer {
            Some(mailer) => mailer.send(to, subject, text, html),
            None => Err(AuthError::internal(
                "no Mailer configured; set one to send verification mail",
            )),
        }
    }

    /// Can mail go out at all? Routes that send mail answer differently
    /// when it cannot.
    pub fn can_mail(&self) -> bool {
        self.mailer.is_some()
    }

    /// The URL a provider redirects back to.
    /// Where the provider sends the browser back. The canonical path is
    /// `/oauth2/callback/:provider`; `/callback/:provider` is kept as an
    /// alias for providers configured the older way.
    pub fn callback_url(&self, provider: &str) -> String {
        format!(
            "{}{PATH_PREFIX}/oauth2/callback/{}",
            self.config.base_url, provider
        )
    }

    // ----------------------------------------------------------- tokens ----

    pub fn sign_access(
        &self,
        user: &User,
        session_id: &str,
        impersonated_by: Option<&str>,
    ) -> String {
        let claims = Claims::access(
            Identity {
                id: &user.id,
                email: &user.email,
                role: &user.role,
                anonymous: user.is_anonymous,
            },
            session_id,
            impersonated_by,
            self.now(),
            self.config.access_ttl_secs,
        );
        crypto::jwt_encode(&claims, &self.config.secret)
    }

    /// The five-minute token that proves a password when the account has
    /// a second factor. It grants nothing but the second-factor routes.
    pub fn sign_pending(&self, user_id: &str) -> String {
        let claims = Claims::two_factor_pending(
            user_id,
            self.now(),
            self.config.two_factor_pending_ttl_secs,
        );
        crypto::jwt_encode(&claims, &self.config.secret)
    }

    /// Verify an access token's signature and expiry.
    pub fn decode(&self, token: &str) -> AuthResult<Claims> {
        crypto::jwt_decode(token, &self.config.secret, self.now())
    }

    /// `{token, refreshToken, user}` with the access token in the header,
    /// the shape every sign-in and refresh returns.
    pub fn token_response(
        &self,
        access: &str,
        refresh: &str,
        user: &User,
        status: u16,
    ) -> Response {
        Response::json(status, &self.token_json(access, refresh, user))
            .with_header(TOKEN_HEADER, access)
    }

    pub fn token_json(&self, access: &str, refresh: &str, user: &User) -> Value {
        Value::Object(object! {
            "token" => s(access),
            "refreshToken" => s(refresh),
            "user" => user.to_json(),
        })
    }

    /// The reply to a sign-in that needs a second factor.
    pub fn pending_response(&self, user_id: &str) -> Response {
        let pending = self.sign_pending(user_id);
        Response::json(
            200,
            &Value::Object(object! {"twoFactorRedirect" => b(true)}),
        )
        .with_header(TOKEN_HEADER, &pending)
    }

    // -------------------------------------------------------- sessions ----

    /// A new session row and its plaintext refresh token.
    pub async fn create_session(
        &self,
        user: &User,
        req: &Request,
        impersonated_by: Option<&str>,
    ) -> AuthResult<(Session, String)> {
        let refresh = crypto::random_token(32);
        let now = self.now();
        let session = Session {
            id: new_id(),
            user_id: user.id.clone(),
            token: crypto::sha256_hex(refresh.as_bytes()),
            expires_at: iso(now + self.config.refresh_ttl_secs),
            ip_address: Some(req.client_ip()),
            user_agent: req
                .header("user-agent")
                .map(|s| s.chars().take(512).collect()),
            impersonated_by: impersonated_by.map(str::to_string),
            created_at: now_iso(),
            updated_at: now_iso(),
            extra: mosql::Object::new(),
        };
        self.store.create_typed(&session).await?;
        Ok((session, refresh))
    }

    /// Sign a user in: session row, access token, refresh token, as a
    /// response ready to return.
    pub async fn issue(
        &self,
        user: &User,
        req: &Request,
        impersonated_by: Option<&str>,
    ) -> AuthResult<Response> {
        let (session, refresh) = self.create_session(user, req, impersonated_by).await?;
        let access = self.sign_access(user, &session.id, impersonated_by);
        Ok(self.token_response(&access, &refresh, user, 200))
    }

    /// Rotate a refresh token: the old digest stops working whether the
    /// caller is honest or a replay.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        req: &Request,
    ) -> AuthResult<(User, Session, String)> {
        let digest = crypto::sha256_hex(refresh_token.as_bytes());
        let existing = self
            .store
            .find_one_typed::<Session>(&[WhereClause::eq("token", s(digest))])
            .await?
            .ok_or_else(|| AuthError::invalid_token("invalid refresh token"))?;
        if existing.expires_at <= now_iso() {
            self.store
                .delete(MODEL_SESSION, &[WhereClause::id(&existing.id)])
                .await?;
            return Err(AuthError::invalid_token("refresh token expired"));
        }
        let user = self
            .user_by_id(&existing.user_id)
            .await?
            .ok_or_else(|| AuthError::invalid_token("session has no user"))?;
        if let Some(reason) = user.ban_in_force(&now_iso()) {
            return Err(AuthError::forbidden("USER_BANNED", reason));
        }
        let (session, refresh) = self
            .create_session(&user, req, existing.impersonated_by.as_deref())
            .await?;
        self.store
            .delete(MODEL_SESSION, &[WhereClause::id(&existing.id)])
            .await?;
        Ok((user, session, refresh))
    }

    pub async fn revoke_session(&self, session_id: &str) -> AuthResult<()> {
        self.store
            .delete(MODEL_SESSION, &[WhereClause::id(session_id)])
            .await
    }

    /// Every session of a user, optionally keeping the current one.
    pub async fn revoke_user_sessions(&self, user_id: &str, keep: Option<&str>) -> AuthResult<()> {
        if let Some(keep) = keep {
            self.store
                .update_many(
                    MODEL_SESSION,
                    &[
                        WhereClause::eq("userId", s(user_id)),
                        WhereClause::or("id", crate::store::FilterOp::Ne, s(keep)),
                    ],
                    expired_update(),
                )
                .await?;
            return Ok(());
        }
        self.store
            .delete_many(MODEL_SESSION, &[WhereClause::eq("userId", s(user_id))])
            .await?;
        Ok(())
    }

    // ----------------------------------------------------------- users ----

    pub async fn user_by_id(&self, id: &str) -> AuthResult<Option<User>> {
        self.store
            .find_one_typed::<User>(&[WhereClause::id(id)])
            .await
    }

    pub async fn user_by_email(&self, email: &str) -> AuthResult<Option<User>> {
        self.store
            .find_one_typed::<User>(&[WhereClause::eq("email", s(email.to_ascii_lowercase()))])
            .await
    }

    pub async fn user_by_username(&self, username: &str) -> AuthResult<Option<User>> {
        self.store
            .find_one_typed::<User>(&[WhereClause::eq(
                "username",
                s(username.to_ascii_lowercase()),
            )])
            .await
    }

    pub async fn save_user(&self, user: &User) -> AuthResult<User> {
        self.store.create_typed(user).await
    }

    /// Apply changed fields to a user row and return the result.
    pub async fn update_user(&self, user_id: &str, mut changes: mosql::Object) -> AuthResult<User> {
        changes.set("updatedAt", s(now_iso()));
        self.store
            .update_typed::<User>(&[WhereClause::id(user_id)], changes)
            .await?
            .ok_or_else(|| AuthError::not_found("USER_NOT_FOUND", "User not found"))
    }

    /// A user, or 404.
    pub async fn require_user(&self, user_id: &str) -> AuthResult<User> {
        self.user_by_id(user_id)
            .await?
            .ok_or_else(|| AuthError::not_found("USER_NOT_FOUND", "User not found"))
    }

    /// Remove a user and everything hanging off it.
    pub async fn delete_user(&self, user_id: &str) -> AuthResult<()> {
        let by_user = [WhereClause::eq("userId", s(user_id))];
        self.store.delete_many(MODEL_SESSION, &by_user).await?;
        self.store.delete_many(MODEL_ACCOUNT, &by_user).await?;
        self.store.delete_many(MODEL_TWO_FACTOR, &by_user).await?;
        self.store
            .delete(MODEL_USER, &[WhereClause::id(user_id)])
            .await
    }

    /// The `credential` account holding the user's password, if any.
    pub async fn credential_account(&self, user_id: &str) -> AuthResult<Option<Account>> {
        self.store
            .find_one_typed::<Account>(&[
                WhereClause::eq("userId", s(user_id)),
                WhereClause::eq("providerId", s(PROVIDER_CREDENTIAL)),
            ])
            .await
    }

    /// Set (or replace) the password on a user's credential account.
    pub async fn set_password(&self, user_id: &str, password: &str) -> AuthResult<Account> {
        check_password(password)?;
        let hash = crypto::hash_password(password, self.config.password_scrypt_n);
        match self.credential_account(user_id).await? {
            Some(account) => {
                let mut changes = mosql::Object::new();
                changes.set("password", s(hash));
                changes.set("updatedAt", s(now_iso()));
                self.store
                    .update_typed::<Account>(&[WhereClause::id(&account.id)], changes)
                    .await?
                    .ok_or_else(|| AuthError::internal("credential account vanished"))
            }
            None => {
                let account = Account {
                    id: new_id(),
                    user_id: user_id.to_string(),
                    account_id: user_id.to_string(),
                    provider_id: PROVIDER_CREDENTIAL.to_string(),
                    access_token: None,
                    refresh_token: None,
                    id_token: None,
                    access_token_expires_at: None,
                    refresh_token_expires_at: None,
                    scope: None,
                    password: Some(hash),
                    created_at: now_iso(),
                    updated_at: now_iso(),
                    extra: mosql::Object::new(),
                };
                self.store.create_typed(&account).await
            }
        }
    }

    /// Verify `password` against the user's hash, if there is one.
    ///
    /// A hash in the old PBKDF2 format is replaced with a scrypt one as
    /// soon as the password is given correctly, so a table written before
    /// the change heals itself one sign-in at a time.
    pub async fn verify_password(&self, user_id: &str, password: &str) -> AuthResult<bool> {
        let Some(account) = self.credential_account(user_id).await? else {
            return Ok(false);
        };
        let Some(stored) = account.password.as_deref() else {
            return Ok(false);
        };
        if !crypto::verify_password(password, stored, self.config.password_scrypt_n) {
            return Ok(false);
        }
        if crypto::password_is_legacy(stored) {
            let fresh = crypto::hash_password(password, self.config.password_scrypt_n);
            let mut changes = mosql::Object::new();
            changes.set("password", s(fresh));
            changes.set("updatedAt", s(now_iso()));
            self.store
                .update_typed::<Account>(&[WhereClause::id(&account.id)], changes)
                .await?;
        }
        Ok(true)
    }

    /// Link a provider account to a user, or update the stored tokens if
    /// the link already exists.
    pub async fn upsert_account(&self, account: &Account) -> AuthResult<Account> {
        let existing = self
            .store
            .find_one_typed::<Account>(&[
                WhereClause::eq("providerId", s(account.provider_id.clone())),
                WhereClause::eq("accountId", s(account.account_id.clone())),
            ])
            .await?;
        if let Some(existing) = existing {
            let mut changes = mosql::Object::new();
            for key in [
                "accessToken",
                "refreshToken",
                "idToken",
                "accessTokenExpiresAt",
                "refreshTokenExpiresAt",
                "scope",
            ] {
                if let Some(value) = account.to_record().get(key) {
                    changes.set(key, value.clone());
                }
            }
            changes.set("updatedAt", s(now_iso()));
            return self
                .store
                .update_typed::<Account>(&[WhereClause::id(&existing.id)], changes)
                .await?
                .ok_or_else(|| AuthError::internal("account vanished"));
        }
        self.store.create_typed(account).await
    }

    pub async fn account_for(
        &self,
        provider_id: &str,
        account_id: &str,
    ) -> AuthResult<Option<Account>> {
        self.store
            .find_one_typed::<Account>(&[
                WhereClause::eq("providerId", s(provider_id)),
                WhereClause::eq("accountId", s(account_id)),
            ])
            .await
    }

    // ---------------------------------------------------- verification ----

    /// Store a token by the SHA-256 of what the user will send back, so
    /// only the digest is in the database. `value` is whatever the route
    /// needs when the token comes home: a user id, or a JSON document.
    pub async fn put_verification(
        &self,
        identifier: &str,
        value: &str,
        ttl_secs: i64,
    ) -> AuthResult<()> {
        self.store
            .delete_many(
                MODEL_VERIFICATION,
                &[WhereClause::eq("identifier", s(identifier))],
            )
            .await?;
        let record = Verification {
            id: new_id(),
            identifier: identifier.to_string(),
            value: value.to_string(),
            expires_at: iso(self.now() + ttl_secs),
            created_at: now_iso(),
            updated_at: now_iso(),
            extra: mosql::Object::new(),
        };
        self.store.create_typed(&record).await?;
        Ok(())
    }

    /// Read a verification value; expired or used tokens are gone for
    /// good. Single-use by construction: the row is deleted on read.
    pub async fn take_verification(&self, identifier: &str) -> AuthResult<Option<String>> {
        let found = self
            .store
            .find_one_typed::<Verification>(&[WhereClause::eq("identifier", s(identifier))])
            .await?;
        let Some(found) = found else {
            return Ok(None);
        };
        self.store
            .delete(MODEL_VERIFICATION, &[WhereClause::id(&found.id)])
            .await?;
        if found.expires_at <= now_iso() {
            return Ok(None);
        }
        Ok(Some(found.value))
    }

    /// The same lookup without consuming, for attempt counting.
    pub async fn peek_verification(&self, identifier: &str) -> AuthResult<Option<Verification>> {
        self.store
            .find_one_typed::<Verification>(&[WhereClause::eq("identifier", s(identifier))])
            .await
    }

    /// Email a link. Text only: a link is a link.
    pub fn mail_link(&self, to: &str, subject: &str, link: &str) -> AuthResult<()> {
        let text = format!(
            "Hello,\n\n{subject}:\n\n{link}\n\nIf you did not request this, you can ignore this email.\n\n— {}",
            self.config.issuer
        );
        self.send_mail(to, subject, &text, None)
    }
}

/// Marks the other sessions of a user as expired in place.
fn expired_update() -> mosql::Object {
    let mut update = mosql::Object::new();
    update.set("expiresAt", s(iso(now_secs() - 1)));
    update.set("updatedAt", s(now_iso()));
    update
}

/// Settings a host passes in when mounting the engine on its own server.
#[derive(Debug, Clone, Default)]
pub struct MountOptions {
    /// Serve under this prefix instead of `/api/auth`.
    pub prefix: Option<String>,
    /// Origins allowed on browser-facing redirects.
    pub allowed_origins: Vec<String>,
}

/// Variables a provider's URLs need, from the environment — exposed so
/// hosts that build a registry themselves see the same rules.
pub fn provider_vars(
    preset: &crate::oauth_presets::ProviderPreset,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for (placeholder, names, _) in preset.url_vars {
        if let Some(value) = names.iter().find_map(|n| lookup(n)) {
            vars.insert(placeholder.to_string(), value);
        }
    }
    if let Some((env, _, _)) = preset.sandbox {
        if let Some(mode) = lookup(env) {
            vars.insert("sandbox".into(), mode);
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_test;
    use crate::store::MemoryStore;

    fn config() -> AuthConfig {
        let mut config = AuthConfig::new("https://app.test", "0123456789abcdef0123456789abcdef");
        // The production count is a 15-second hash per password; tests only
        // need the format.
        config.password_scrypt_n = 1024;
        config
    }

    fn auth() -> Auth {
        Auth::new(Arc::new(MemoryStore::new()), config())
    }

    fn user(email: &str) -> User {
        User {
            id: "u1".into(),
            name: "Ada".into(),
            email: email.into(),
            email_verified: true,
            image: None,
            username: None,
            display_username: None,
            is_anonymous: false,
            two_factor_enabled: false,
            role: "user".into(),
            banned: false,
            ban_reason: None,
            ban_expires: None,
            created_at: now_iso(),
            updated_at: now_iso(),
            extra: mosql::Object::new(),
        }
    }

    async_test! {
    async fn sessions_issue_rotate_and_reject_replay() {
        let auth = auth();
        let ada = auth.save_user(&user("ada@x.co")).await.unwrap();
        let req = Request::new("POST", "/x").with_header("User-Agent", "test");
        let (session, refresh) = auth.create_session(&ada, &req, None).await.unwrap();
        assert_eq!(session.token, crypto::sha256_hex(refresh.as_bytes()));
        assert!(auth.user_by_id(&ada.id).await.unwrap().is_some());

        // A refresh rotates: new row, old digest gone.
        let (user2, session2, refresh2) = auth.refresh(&refresh, &req).await.unwrap();
        assert_eq!(user2.id, ada.id);
        assert_ne!(session2.id, session.id);
        assert_ne!(refresh2, refresh);
        let err = auth.refresh(&refresh, &req).await.unwrap_err();
        assert_eq!(err.status, 401);
        assert_eq!(err.code, "INVALID_TOKEN");
        // Still exactly one live session.
        let sessions = auth
            .store
            .find_many_typed::<Session>(crate::store::FindManyQuery::new(vec![WhereClause::eq("userId", s(&ada.id))]))
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
    }
    }

    async_test! {
    async fn tokens_carry_identity_and_verify() {
        let auth = auth();
        let ada = user("ada@x.co");
        let access = auth.sign_access(&ada, "sess1", Some("admin1"));
        let claims = auth.decode(&access).unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.sid.as_deref(), Some("sess1"));
        assert_eq!(claims.role.as_deref(), Some("user"));
        assert_eq!(claims.imp.as_deref(), Some("admin1"));
        assert!(!claims.is_two_factor_pending());
        let tampered = format!("{}x", &access[..access.len() - 2]);
        assert_eq!(auth.decode(&tampered).unwrap_err().status, 401);
        let pending = auth.sign_pending("u1");
        let claims = auth.decode(&pending).unwrap();
        assert!(claims.is_two_factor_pending());
        assert_eq!(claims.sid, None);
    }
    }

    async_test! {
    async fn passwords_and_credential_accounts() {
        let auth = auth();
        let ada = auth.save_user(&user("ada@x.co")).await.unwrap();
        assert!(auth.credential_account(&ada.id).await.unwrap().is_none());
        let short = auth.set_password(&ada.id, "short").await.unwrap_err();
        assert_eq!(short.status, 400);
        let account = auth.set_password(&ada.id, "correct horse battery").await.unwrap();
        assert_eq!(account.provider_id, PROVIDER_CREDENTIAL);
        // The hash is in Better Auth's format: salt, then key.
        let stored = account.password.as_deref().unwrap();
        let (salt, key) = stored.split_once(':').expect("two halves");
        assert_eq!(salt.len(), 32);
        assert_eq!(key.len(), 128);
        assert!(auth.verify_password(&ada.id, "correct horse battery").await.unwrap());
        assert!(!auth.verify_password(&ada.id, "wrong").await.unwrap());
        // Replacing keeps one account.
        auth.set_password(&ada.id, "another long password").await.unwrap();
        let accounts = auth
            .store
            .find_many_typed::<Account>(crate::store::FindManyQuery::new(vec![WhereClause::eq("userId", s(&ada.id))]))
            .await
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(auth.verify_password(&ada.id, "another long password").await.unwrap());
    }
    }

    async_test! {
    async fn verification_tokens_are_single_use_and_expire() {
        let auth = auth();
        let id = format!("pw-reset:{}", crypto::sha256_hex(b"tok"));
        auth.put_verification(&id, "u1", 60).await.unwrap();
        assert_eq!(auth.take_verification(&id).await.unwrap().as_deref(), Some("u1"));
        assert_eq!(auth.take_verification(&id).await.unwrap(), None);
        auth.put_verification(&id, "u1", -1).await.unwrap();
        assert_eq!(auth.take_verification(&id).await.unwrap(), None);
        // Only the digest is stored.
        auth.put_verification(&id, "u1", 60).await.unwrap();
        let rows = auth
            .store
            .find_many(MODEL_VERIFICATION, crate::store::FindManyQuery::default())
            .await
            .unwrap();
        let rows: Vec<Value> = rows.into_iter().map(Value::Object).collect();
        assert!(!mosql::to_json(&Value::Array(rows)).contains("tok\""));
    }
    }

    #[test]
    fn config_reads_env() {
        let config = AuthConfig::from_env_with(|k| match k {
            "BETTER_AUTH_SECRET" => Some("0123456789abcdef0123456789abcdef".into()),
            "BETTER_AUTH_URL" => Some("https://app.example.com/".into()),
            "BETTER_AUTH_REQUIRE_EMAIL_VERIFICATION" => Some("true".into()),
            "ACCESS_TTL_SECS" => Some("60".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(config.base_url, "https://app.example.com");
        assert_eq!(config.issuer, "app.example.com");
        assert!(config.require_email_verification);
        assert_eq!(config.access_ttl_secs, 60);
        assert_eq!(config.refresh_ttl_secs, 30 * 24 * 60 * 60);
        let err = AuthConfig::from_env_with(|_| Some("short".into()))
            .err()
            .unwrap();
        assert!(err.message.contains("32"));
        let err = AuthConfig::from_env_with(|k| {
            (k == "BETTER_AUTH_SECRET").then(|| "0123456789abcdef0123456789abcdef".into())
        })
        .err()
        .unwrap();
        assert!(err.message.contains("BETTER_AUTH_URL"));
    }

    #[test]
    fn urls_split_into_host_port_path() {
        assert_eq!(
            split_url("http://a.test/x?y=1").unwrap(),
            ("a.test".into(), 80, "/x?y=1".into())
        );
        assert_eq!(
            split_url("https://a.test:8443").unwrap(),
            ("a.test".into(), 8443, "/".into())
        );
        assert!(split_url("ftp://a.test").is_err());
    }

    #[test]
    fn plain_client_round_trips_against_the_server() {
        use crate::http::{serve_on, status_text};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            serve_on(listener, 1, |req| {
                Response::json(
                    201,
                    &Value::Object(object! {
                        "method" => s(req.method.clone()),
                        "echo" => req.json().unwrap_or(Value::Null),
                    }),
                )
            })
            .unwrap();
        });
        let reply = PlainHttpClient
            .request(&HttpRequest {
                method: "POST".into(),
                url: format!("http://{addr}/oauth/token"),
                headers: vec![("content-type".into(), "application/json".into())],
                body: br#"{"a":1}"#.to_vec(),
            })
            .unwrap();
        assert_eq!(reply.status, 201);
        let json = reply.json().unwrap();
        assert_eq!(json.get("method"), Some(&Value::str("POST")));
        assert_eq!(
            json.get("echo").and_then(|v| v.get("a")),
            Some(&Value::Int(1))
        );
        let err = PlainHttpClient
            .request(&HttpRequest {
                method: "GET".into(),
                url: "https://accounts.google.com/x".into(),
                headers: vec![],
                body: vec![],
            })
            .unwrap_err();
        assert!(err.message.contains("TLS"));
        assert_eq!(status_text(201), "Created");
    }
}
