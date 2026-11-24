//! OAuth: the redirect out, the callback back, and the user that comes
//! home with it.
//!
//! The state parameter carries the whole round trip: the provider, the
//! PKCE verifier, where to send the browser afterwards, and — for an
//! anonymous caller or an explicit link — which user to attach the
//! account to. It is random, stored server-side under its hash, and
//! consumed on use.
//!
//! The provider's access token is never shown to the browser. What the
//! browser gets is *our* token pair, in the redirect query.

use std::sync::Arc;

use mosql::Value;

use crate::auth::{Auth, HttpRequest};
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::{
    authenticate, optional, redirect_with_error, redirect_with_tokens, validate_redirect,
};
use crate::models::*;
use crate::oauth_presets::{
    extract_profile, Method, ProfileSource, ProviderConfig, UserinfoRequest,
};
use crate::object;
use crate::routes::{body, core, field, json, ok_status, opt_bool, opt_field, redirect_target};
use crate::store::{AuthStoreExt, FindManyQuery, WhereClause};

fn provider_not_found(id: &str) -> AuthError {
    AuthError::not_found(
        "PROVIDER_NOT_FOUND",
        format!("{id} is not a configured provider"),
    )
}

/// What the state row remembers.
#[derive(Debug, Clone, PartialEq)]
struct Flow {
    provider: String,
    code_verifier: Option<String>,
    redirect_uri: String,
    callback_url: Option<String>,
    error_callback_url: Option<String>,
    link_user_id: Option<String>,
}

impl Flow {
    fn to_json(&self) -> String {
        let opt = |v: &Option<String>| {
            v.as_deref()
                .map(core::json_string)
                .unwrap_or_else(|| "null".into())
        };
        format!(
            r#"{{"provider":{},"codeVerifier":{},"redirectURI":{},"callbackURL":{},"errorCallbackURL":{},"linkUserId":{}}}"#,
            core::json_string(&self.provider),
            opt(&self.code_verifier),
            core::json_string(&self.redirect_uri),
            opt(&self.callback_url),
            opt(&self.error_callback_url),
            opt(&self.link_user_id)
        )
    }

    fn parse(text: &str) -> AuthResult<Flow> {
        let data = core::parse_object(text)?;
        let get = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_string);
        Ok(Flow {
            provider: get("provider")
                .ok_or_else(|| AuthError::internal("state has no provider"))?,
            code_verifier: get("codeVerifier"),
            redirect_uri: get("redirectURI")
                .ok_or_else(|| AuthError::internal("state has no redirect URI"))?,
            callback_url: get("callbackURL"),
            error_callback_url: get("errorCallbackURL"),
            link_user_id: get("linkUserId"),
        })
    }
}

fn scopes_of(body: &Value) -> Vec<String> {
    body.get("scopes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the authorization redirect and remember the flow.
async fn begin(auth: &Arc<Auth>, req: &Request, force_link: bool) -> AuthResult<Response> {
    let body = body(req)?;
    let provider_id = field(&body, "provider").or_else(|_| field(&body, "providerId"))?;
    let config = auth
        .oauth
        .get(&provider_id)
        .ok_or_else(|| provider_not_found(&provider_id))?
        .clone();
    if auth.http.is_none() {
        return Err(AuthError::internal(
            "no HttpClient configured; OAuth needs outbound HTTP",
        ));
    }
    let callback_url = redirect_target(auth, req, &body, "callbackURL")?;
    let error_callback_url = redirect_target(auth, req, &body, "errorCallbackURL")?;
    let login_hint = opt_field(&body, "loginHint");
    let prompt = opt_field(&body, "prompt");
    let scopes = scopes_of(&body);
    let link = force_link || opt_bool(&body, "link") == Some(true);

    let caller = optional(auth, req);
    let link_user_id = if link {
        let who = caller.ok_or_else(|| {
            AuthError::unauthorized("UNAUTHORIZED", "Sign in before linking an account")
        })?;
        if who.is_anonymous() {
            return Err(AuthError::bad_request(
                "ANONYMOUS_USER",
                "Anonymous users cannot link accounts",
            ));
        }
        Some(who.user_id().to_string())
    } else {
        // An anonymous visitor signing in with a provider keeps their id.
        caller
            .filter(|w| w.is_anonymous())
            .map(|w| w.user_id().to_string())
    };

    let (code_verifier, challenge) = if config.pkce {
        let (verifier, challenge) = crate::crypto::pkce_pair();
        (Some(verifier), Some(challenge))
    } else {
        (None, None)
    };
    let state = core::new_token();
    let redirect_uri = auth.callback_url(&config.id);
    let flow = Flow {
        provider: config.id.clone(),
        code_verifier,
        redirect_uri: redirect_uri.clone(),
        callback_url,
        error_callback_url,
        link_user_id,
    };
    auth.put_verification(
        &core::token_key(core::V_STATE, &state),
        &flow.to_json(),
        auth.config.oauth_state_ttl_secs,
    )
    .await?;
    let url = config.authorization_url(
        &redirect_uri,
        &state,
        challenge.as_deref(),
        &scopes,
        login_hint.as_deref(),
        prompt.as_deref(),
    );
    Ok(json(
        200,
        &Value::Object(object! {
            "url" => s(url),
            "redirect" => b(true),
        }),
    ))
}

/// `POST /sign-in/social` (and `/sign-in/oauth2`) → `{url, redirect: true}`.
pub async fn sign_in(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    begin(auth, req, false).await
}

/// `POST /oauth2/link` → the same, for a signed-in user.
pub async fn link(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    begin(auth, req, true).await
}

/// The provider's answer: `GET` (redirect) or `POST` (Apple's `form_post`).
pub async fn callback(auth: &Arc<Auth>, req: &Request, provider: &str) -> AuthResult<Response> {
    let Some(config) = auth.oauth.get(provider) else {
        return Err(provider_not_found(provider));
    };
    let config = config.clone();
    let form = if req.is_form() {
        req.form()
    } else {
        Vec::new()
    };
    let param = |name: &str| {
        form.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .or_else(|| req.query_param(name).map(str::to_string))
    };

    // Where to send the browser: validated query, else the stored one.
    let mut callback_url = match req.query_param("callbackURL") {
        Some(c) => Some(validate_redirect(auth, c)?),
        None => None,
    };
    let mut error_callback_url: Option<String> = None;

    let fail = |error: AuthError, error_callback: Option<String>, callback: Option<String>| {
        match error_callback.or(callback) {
            Some(target) => redirect_with_error(&target, &error),
            None => Response::error(&error),
        }
    };

    if let Some(error_code) = param("error") {
        let error = AuthError::bad_request(
            "OAUTH_ACCESS_DENIED",
            param("error_description").unwrap_or(error_code),
        );
        return Ok(fail(
            error,
            error_callback_url.clone(),
            callback_url.clone(),
        ));
    }
    let Some(code) = param("code").filter(|c| !c.is_empty()) else {
        return Err(AuthError::invalid_request(
            "the provider did not return a code",
        ));
    };
    let Some(state) = param("state") else {
        return Err(AuthError::invalid_request(
            "the provider did not return state",
        ));
    };
    let stored = auth
        .take_verification(&core::token_key(core::V_STATE, &state))
        .await?;
    let Some(stored) = stored else {
        return Err(AuthError::bad_request(
            "INVALID_STATE",
            "Unknown or expired state; start sign-in again",
        ));
    };
    let flow = Flow::parse(&stored)?;
    if flow.provider != config.id {
        return Err(AuthError::bad_request(
            "INVALID_STATE",
            "State was issued for another provider",
        ));
    }
    if callback_url.is_none() {
        callback_url = flow.callback_url.clone();
    }
    error_callback_url = flow.error_callback_url.clone();

    let outcome = finish(auth, req, &config, &flow, &code, param("user").as_deref()).await;
    match outcome {
        Ok((user, is_new)) => {
            let (session, refresh) = auth.create_session(&user, req, None).await?;
            let access = auth.sign_access(&user, &session.id, None);
            let _ = is_new;
            match callback_url {
                Some(target) => Ok(redirect_with_tokens(&target, &access, &refresh)),
                None => Ok(auth.token_response(&access, &refresh, &user, 200)),
            }
        }
        Err(e) => Ok(fail(e, error_callback_url, callback_url)),
    }
}

/// Exchange the code, read the profile, find or create the user, link the
/// account.
async fn finish(
    auth: &Auth,
    _req: &Request,
    config: &ProviderConfig,
    flow: &Flow,
    code: &str,
    apple_user: Option<&str>,
) -> AuthResult<(User, bool)> {
    // ---- code for tokens ------------------------------------------------
    let (_, body, headers) =
        config.token_request(code, &flow.redirect_uri, flow.code_verifier.as_deref());
    let response = auth.send(&HttpRequest {
        method: "POST".into(),
        url: config.token_url.clone(),
        headers,
        body,
    })?;
    let status = response.status;
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let token = match mosql::parse_json(&text) {
        Ok(v) => v,
        // Form-encoded answers, as some providers send.
        Err(_) => {
            let mut o = mosql::Object::new();
            for (k, v) in crate::http::parse_query(&text) {
                o.set(k, Value::str(v));
            }
            Value::Object(o)
        }
    };
    if let Some(error) = token.get("error").and_then(Value::as_str) {
        let detail = token
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error)
            .to_string();
        return Err(AuthError::bad_request(
            "OAUTH_TOKEN_FAILED",
            format!("Token exchange failed ({status}): {detail}"),
        ));
    }
    let access_token = token
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AuthError::bad_request(
                "OAUTH_TOKEN_FAILED",
                format!("Token exchange gave no access token ({status})"),
            )
        })?
        .to_string();
    let id_token = token
        .get("id_token")
        .and_then(Value::as_str)
        .map(str::to_string);

    // ---- profile ----------------------------------------------------------
    let id_claims = id_token.as_deref().and_then(crate::crypto::jwt_payload);
    let (profile_doc, source) =
        fetch_profile(auth, config, &access_token, &token, id_claims.as_ref()).await?;
    let _ = source;
    let mut email_doc: Option<Value> = None;
    if config.email_endpoint.is_some() && first_email(&profile_doc, config).is_none() {
        email_doc = fetch_email(auth, config, &access_token).await?;
    }
    let mut profile = extract_profile(
        config,
        &token,
        &profile_doc,
        email_doc.as_ref().map(|d| (d, true)),
    );
    // Apple sends the name only on the first sign-in, as a form field.
    if let Some(user_json) = apple_user.and_then(|u| mosql::parse_json(u).ok()) {
        let first = user_json
            .get("name")
            .and_then(|n| n.get("firstName"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let last = user_json
            .get("name")
            .and_then(|n| n.get("lastName"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let full = format!("{first} {last}").trim().to_string();
        if !full.is_empty() {
            profile.name = full;
        }
    }
    if profile.id.is_empty() {
        return Err(AuthError::internal(format!(
            "{} returned no account id",
            config.id
        )));
    }
    let synthetic = profile
        .email
        .as_deref()
        .is_some_and(|e| e.ends_with(crate::oauth_presets::SYNTHETIC_EMAIL_DOMAIN));

    // ---- which user ---------------------------------------------------------
    let mut is_new = false;
    let user = if let Some(account) = auth.account_for(&config.id, &profile.id).await? {
        auth.require_user(&account.user_id).await?
    } else if let Some(link_id) = &flow.link_user_id {
        // Attach to the given user: an anonymous upgrade, or an explicit link.
        let user = auth.require_user(link_id).await?;
        let mut changes = mosql::Object::new();
        if user.is_anonymous
            || user
                .email
                .ends_with(crate::routes::anonymous::ANONYMOUS_DOMAIN)
        {
            changes.set("isAnonymous", b(false));
            changes.set(
                "email",
                s(profile.email.clone().unwrap_or(user.email.clone())),
            );
            changes.set("emailVerified", b(profile.email_verified));
            if user.name.is_empty() || user.name == "Anonymous" {
                changes.set("name", s(profile.name.clone()));
            }
            if user.image.is_none() {
                if let Some(image) = &profile.image {
                    changes.set("image", s(image.clone()));
                }
            }
        }
        if changes.iter().next().is_some() {
            auth.update_user(&user.id, changes).await?
        } else {
            user
        }
    } else if !synthetic {
        match auth
            .user_by_email(profile.email.as_deref().unwrap_or_default())
            .await?
        {
            Some(user) => user,
            None => {
                is_new = true;
                auth.save_user(&new_user(
                    &profile.name,
                    profile.email.as_deref().unwrap_or_default(),
                    profile.image.as_deref(),
                    profile.email_verified,
                ))
                .await?
            }
        }
    } else {
        is_new = true;
        auth.save_user(&new_user(
            &profile.name,
            profile.email.as_deref().unwrap_or_default(),
            profile.image.as_deref(),
            profile.email_verified,
        ))
        .await?
    };

    // ---- account row --------------------------------------------------------
    let expires = token
        .get("expires_in")
        .and_then(Value::as_f64)
        .map(|secs| iso(now_secs() + secs as i64));
    let account = Account {
        id: new_id(),
        user_id: user.id.clone(),
        account_id: profile.id.clone(),
        provider_id: config.id.clone(),
        access_token: Some(access_token),
        refresh_token: token
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        id_token,
        access_token_expires_at: expires,
        refresh_token_expires_at: None,
        scope: token
            .get("scope")
            .and_then(Value::as_str)
            .map(str::to_string),
        password: None,
        created_at: now_iso(),
        updated_at: now_iso(),
        extra: mosql::Object::new(),
    };
    auth.upsert_account(&account).await?;
    Ok((user, is_new))
}

fn new_user(name: &str, email: &str, image: Option<&str>, verified: bool) -> User {
    User {
        id: new_id(),
        name: if name.trim().is_empty() {
            email.split('@').next().unwrap_or("user").to_string()
        } else {
            name.to_string()
        },
        email: email.to_ascii_lowercase(),
        email_verified: verified,
        image: image.map(str::to_string),
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

fn first_email(profile: &Value, config: &ProviderConfig) -> Option<String> {
    config
        .email_paths
        .iter()
        .filter_map(|p| lookup(profile, p).and_then(Value::as_str))
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

/// Fetch the profile document per the preset's `profile` setting.
async fn fetch_profile(
    auth: &Auth,
    config: &ProviderConfig,
    access_token: &str,
    token: &Value,
    id_claims: Option<&Value>,
) -> AuthResult<(Value, &'static str)> {
    let via_userinfo = || async {
        match config.userinfo_request(access_token) {
            Some(UserinfoRequest {
                method,
                url,
                headers,
                body,
            }) => {
                let reply = auth.send(&HttpRequest {
                    method: match method {
                        Method::Get => "GET",
                        Method::Post => "POST",
                    }
                    .into(),
                    url,
                    headers,
                    body,
                })?;
                if reply.status >= 400 {
                    return Err(AuthError::bad_request(
                        "OAUTH_PROFILE_FAILED",
                        format!(
                            "{} returned {} for the user profile",
                            config.id, reply.status
                        ),
                    ));
                }
                Ok(reply.json()?)
            }
            None => Err(AuthError::internal(format!(
                "{} has no userinfo endpoint",
                config.id
            ))),
        }
    };
    match config.profile {
        ProfileSource::Userinfo => Ok((via_userinfo().await?, "userinfo")),
        ProfileSource::IdToken => {
            let claims = id_claims.ok_or_else(|| {
                AuthError::bad_request(
                    "OAUTH_PROFILE_FAILED",
                    format!("{} returned no id_token", config.id),
                )
            })?;
            Ok((claims.clone(), "id_token"))
        }
        ProfileSource::IdTokenOrUserinfo => match id_claims {
            Some(claims)
                if first_email(claims, config).is_some()
                    || lookup(claims, &config.id_paths[0]).is_some() =>
            {
                Ok((claims.clone(), "id_token"))
            }
            _ => Ok((via_userinfo().await?, "userinfo")),
        },
        ProfileSource::UserinfoOrIdToken => match via_userinfo().await {
            Ok(doc) => Ok((doc, "userinfo")),
            Err(e) => match id_claims {
                Some(claims) => Ok((claims.clone(), "id_token")),
                None => Err(e),
            },
        },
        ProfileSource::TokenResponse => {
            if lookup(token, &config.id_paths[0]).is_some() {
                return Ok((token.clone(), "token"));
            }
            Ok((via_userinfo().await?, "userinfo"))
        }
    }
}

/// GitHub's `/user/emails`, Twitter's confirmed address.
async fn fetch_email(
    auth: &Auth,
    config: &ProviderConfig,
    access_token: &str,
) -> AuthResult<Option<Value>> {
    let Some(endpoint) = &config.email_endpoint else {
        return Ok(None);
    };
    let reply = auth.send(&HttpRequest {
        method: "GET".into(),
        url: endpoint.url.clone(),
        headers: vec![
            ("authorization".into(), format!("Bearer {access_token}")),
            ("accept".into(), "application/json".into()),
            ("user-agent".into(), "better-auth".into()),
        ],
        body: Vec::new(),
    })?;
    if reply.status >= 400 {
        return Ok(None);
    }
    Ok(Some(reply.json()?))
}

/// `GET /list-accounts` → the caller's linked providers, without tokens.
pub async fn list_accounts(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let accounts = auth
        .store
        .find_many_typed::<Account>(FindManyQuery::new(vec![WhereClause::eq(
            "userId",
            s(who.user_id()),
        )]))
        .await?;
    let items: Vec<Value> = accounts
        .iter()
        .map(|a| {
            let mut o = object! {
                "id" => s(a.id.clone()),
                "providerId" => s(a.provider_id.clone()),
                "accountId" => s(a.account_id.clone()),
                "createdAt" => s(a.created_at.clone()),
                "updatedAt" => s(a.updated_at.clone()),
            };
            if let Some(scope) = &a.scope {
                o.set("scope", s(scope.clone()));
            }
            if let Some(expires) = &a.access_token_expires_at {
                o.set("accessTokenExpiresAt", s(expires.clone()));
            }
            Value::Object(o)
        })
        .collect();
    Ok(json(
        200,
        &Value::Object(object! {"accounts" => Value::Array(items)}),
    ))
}

/// `POST /unlink-account {accountId}` → `{status: true}`. The credential
/// account stays: it is how the user signs in without a provider.
pub async fn unlink_account(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let account_id = field(&body, "accountId")?;
    let account = auth
        .store
        .find_one_typed::<Account>(&[
            WhereClause::id(&account_id),
            WhereClause::eq("userId", s(who.user_id())),
        ])
        .await?
        .ok_or_else(|| AuthError::not_found("ACCOUNT_NOT_FOUND", "Account not found"))?;
    if account.provider_id == PROVIDER_CREDENTIAL {
        return Err(AuthError::bad_request(
            "CANNOT_UNLINK_CREDENTIAL",
            "The password account cannot be unlinked",
        ));
    }
    auth.store
        .delete(MODEL_ACCOUNT, &[WhereClause::id(&account.id)])
        .await?;
    ok_status()
}
