//! The 36 built-in OAuth providers, and the registry that turns environment
//! variables into configured providers.
//!
//! Every preset is a statement of fact about one provider's OAuth2 endpoints
//! and the shape of its user profile: URLs, default scopes, how the client
//! authenticates at the token endpoint, which dot-paths of the profile hold
//! the id, name, email and picture. The definitions follow Better Auth's
//! social providers so that an app migrating from it keeps working with the
//! same credentials.
//!
//! At runtime a preset plus its credentials becomes a [`ProviderConfig`],
//! and the OAuth route module does the talking: it builds the authorization
//! redirect from [`ProviderConfig::authorization_url`], exchanges the code,
//! fetches the profile with [`userinfo_request`], and reads it back with
//! [`extract_profile`].

use std::collections::BTreeMap;

use mosql::Value;

use crate::http::{form_encode, percent_encode};
use crate::models::lookup;

/// Where the client credentials go at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAuth {
    /// HTTP Basic `Authorization` header (RFC 6749 §2.3.1).
    Basic,
    /// `client_id` and `client_secret` in the request body.
    Body,
}

/// The wire format of the token request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenBody {
    /// `application/x-www-form-urlencoded` — what OAuth2 requires.
    Form,
    /// `application/json` — Notion's preference.
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// How the userinfo endpoint is authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserinfoAuth {
    /// `Authorization: Bearer <access_token>`.
    Bearer,
    /// `access_token` and `client_id` in a form body (VK).
    FormBody,
    /// `access_token` in the query string (WeChat).
    Query,
}

/// Where the profile document comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    /// GET or POST the userinfo endpoint with the access token.
    Userinfo,
    /// Decode the `id_token` the token endpoint returned.
    IdToken,
    /// The `id_token` when there is one, the userinfo endpoint otherwise.
    IdTokenOrUserinfo,
    /// The userinfo endpoint, falling back to the `id_token`.
    UserinfoOrIdToken,
    /// The token response itself, when no id_token is present (Notion's
    /// `owner.user`).
    TokenResponse,
}

/// Providers whose picture needs a step beyond reading a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFixup {
    None,
    /// `https://cdn.discordapp.com/avatars/{id}/{avatar}.{png|gif}`.
    DiscordAvatar,
    /// Everything before the first `?` (Reddit's sized variants).
    StripQuery,
}

/// A second request that only exists to find an email address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailEndpoint {
    pub url: &'static str,
    /// The body is an array of objects; pick the one with `primary == true`,
    /// else the first.
    pub primary_array: bool,
    /// Dot-path to the address within the chosen object.
    pub path: &'static str,
    /// Dot-path to a verified flag within the chosen object.
    pub verified_path: Option<&'static str>,
}

/// One provider as shipped. Pure data; nothing here touches the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub client_id_env: &'static str,
    pub client_secret_env: &'static str,
    pub authorization_url: &'static str,
    pub token_url: &'static str,
    pub userinfo_url: Option<&'static str>,
    pub userinfo_method: Method,
    pub userinfo_auth: UserinfoAuth,
    /// Extra static headers for the userinfo request.
    pub userinfo_headers: &'static [(&'static str, &'static str)],
    /// A header carrying the client id (Twitch's `Client-Id`).
    pub userinfo_client_id_header: Option<&'static str>,
    /// A fixed body for the userinfo request (Linear's GraphQL query).
    pub userinfo_body: Option<&'static str>,
    /// Query parameters appended to the userinfo URL.
    pub userinfo_query: &'static [(&'static str, &'static str)],
    pub scopes: &'static [&'static str],
    /// What joins several scopes. OAuth2 uses a space; WeChat a comma.
    pub scope_separator: &'static str,
    pub pkce: bool,
    pub token_auth: TokenAuth,
    pub token_body: TokenBody,
    /// The authorization request's client parameter: `client_id`, or
    /// TikTok's `client_key`.
    pub client_id_param: &'static str,
    /// The token request's secret parameter: `client_secret`, or WeChat's
    /// `secret`.
    pub client_secret_param: &'static str,
    /// How the token request is sent. `Get` puts the fields in the query
    /// string, as WeChat wants.
    pub token_method: Method,
    /// Extra fields in the token request.
    pub token_extra_fields: &'static [(&'static str, &'static str)],
    /// Standard fields the token request leaves out. WeChat wants neither
    /// `redirect_uri` nor `code_verifier`.
    pub token_omits: &'static [&'static str],
    /// A fragment on the authorization URL: WeChat needs `#wechat_redirect`.
    pub auth_url_fragment: Option<&'static str>,
    /// Userinfo query parameters taken from the token response. WeChat's
    /// userinfo endpoint needs the `openid` that came with the token.
    pub userinfo_query_from_token: &'static [&'static str],
    pub response_type: &'static str,
    pub response_mode: Option<&'static str>,
    /// The `prompt` parameter's default when the caller names none.
    pub prompt: Option<&'static str>,
    /// Extra parameters on the authorization request.
    pub auth_params: &'static [(&'static str, &'static str)],
    /// Placeholders in the URLs above, each filled from the first
    /// environment variable present, else the default. A missing variable
    /// with no default leaves the provider unconfigured.
    pub url_vars: &'static [(&'static str, &'static [&'static str], Option<&'static str>)],
    /// `(env var, api host, www host)` — when the env var is `sandbox`, the
    /// two host placeholders resolve to the sandbox values instead.
    pub sandbox: Option<(&'static str, &'static str, &'static str)>,
    pub profile: ProfileSource,
    pub id_paths: &'static [&'static str],
    pub name_paths: &'static [&'static str],
    /// Two paths joined with a space make the display name.
    pub name_join: Option<(&'static str, &'static str)>,
    pub email_paths: &'static [&'static str],
    pub image_paths: &'static [&'static str],
    pub image_fixup: ImageFixup,
    pub verified_paths: &'static [&'static str],
    pub email_endpoint: Option<EmailEndpoint>,
    /// An email field on the token response itself (VK).
    pub token_email_path: Option<&'static str>,
}

#[allow(clippy::too_many_arguments)]
macro_rules! standard {
    (
        $env:literal, $id:literal, $name:literal, $auth:literal, $token:literal, $userinfo:expr,
        scopes [$($scope:literal),*], id [$($idp:literal),*], name [$($namep:literal),*],
        email [$($emailp:literal),*], image [$($imagep:literal),*], verified [$($verp:literal),*]
        $(, extra [$($extra_env:literal),*])?
    ) => {
        ProviderPreset {
            id: $id,
            display_name: $name,
            client_id_env: concat!($env, "_CLIENT_ID"),
            client_secret_env: concat!($env, "_CLIENT_SECRET"),
            authorization_url: $auth,
            token_url: $token,
            userinfo_url: $userinfo,
            userinfo_method: Method::Get,
            userinfo_auth: UserinfoAuth::Bearer,
            userinfo_headers: &[],
            userinfo_client_id_header: None,
            userinfo_body: None,
            userinfo_query: &[],
            scopes: &[$($scope),*],
            scope_separator: " ",
            pkce: true,
            token_auth: TokenAuth::Body,
            token_body: TokenBody::Form,
            client_id_param: "client_id",
            client_secret_param: "client_secret",
            token_method: Method::Post,
            token_extra_fields: &[],
            token_omits: &[],
            auth_url_fragment: None,
            userinfo_query_from_token: &[],
            response_type: "code",
            response_mode: None,
            prompt: None,
            auth_params: &[],
            url_vars: &[],
            sandbox: None,
            profile: ProfileSource::Userinfo,
            id_paths: &[$($idp),*],
            name_paths: &[$($namep),*],
            name_join: None,
            email_paths: &[$($emailp),*],
            image_paths: &[$($imagep),*],
            image_fixup: ImageFixup::None,
            verified_paths: &[$($verp),*],
            email_endpoint: None,
            token_email_path: None,
        }
    };
}

/// The domain used for addresses invented for providers that do not
/// return one. It is reserved by RFC 2606 and so cannot collide with a
/// real mailbox.
pub const SYNTHETIC_EMAIL_DOMAIN: &str = "oauth.local";

/// A placeholder address for an account the provider gave no email for.
/// Lower-cased so it compares equal to a typed address.
pub fn synthetic_email(provider: &str, account_id: &str) -> String {
    format!(
        "{}_{}@{}",
        provider.to_ascii_lowercase(),
        account_id,
        SYNTHETIC_EMAIL_DOMAIN
    )
}

pub static PRESETS: [ProviderPreset; 36] = [
    // Id token first: it is signed by Google and carries the verified flag.
    ProviderPreset {
        profile: ProfileSource::IdToken,
        ..standard!(
            "GOOGLE", "google", "Google",
            "https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", None,
            scopes ["email", "profile", "openid"],
            id ["sub"], name ["name", "given_name"], email ["email"], image ["picture"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        email_endpoint: Some(EmailEndpoint {
            url: "https://api.github.com/user/emails",
            primary_array: true,
            path: "email",
            verified_path: Some("verified"),
        }),
        userinfo_headers: &[("user-agent", "better-auth")],
        ..standard!(
            "GITHUB", "github", "GitHub",
            "https://github.com/login/oauth/authorize", "https://github.com/login/oauth/access_token",
            Some("https://api.github.com/user"),
            scopes ["read:user", "user:email"],
            id ["id"], name ["name", "login"], email ["email"], image ["avatar_url"], verified ["email_verified"]
        )
    },
    // Apple posts `code` and `id_token` to the callback as a form, and sends
    // the user's name only in that first POST, as a JSON string under `user`.
    ProviderPreset {
        profile: ProfileSource::IdToken,
        response_mode: Some("form_post"),
        response_type: "code id_token",
        ..standard!(
            "APPLE", "apple", "Apple",
            "https://appleid.apple.com/auth/authorize", "https://appleid.apple.com/auth/token", None,
            scopes ["email", "name"],
            id ["sub"], name ["name"], email ["email"], image [], verified ["email_verified"]
        )
    },
    standard!(
        "ATLASSIAN", "atlassian", "Atlassian",
        "https://auth.atlassian.com/authorize", "https://auth.atlassian.com/oauth/token",
        Some("https://api.atlassian.com/me"),
        scopes ["read:jira-user", "offline_access"],
        id ["account_id"], name ["name"], email ["email"], image ["picture"], verified []
    ),
    // `{domain}` is the Cognito hosted UI domain, `{region}` and
    // `{user_pool}` only matter to the issuer check, which the route does
    // not perform; the domain is the one variable that is required.
    ProviderPreset {
        profile: ProfileSource::IdTokenOrUserinfo,
        url_vars: &[
            ("domain", &["COGNITO_DOMAIN", "AMAZON_COGNITO_DOMAIN"], None),
            ("region", &["COGNITO_REGION", "AMAZON_COGNITO_REGION"], None),
            (
                "user_pool",
                &["COGNITO_USER_POOL_ID", "AMAZON_COGNITO_USER_POOL_ID"],
                None,
            ),
        ],
        ..standard!(
            "COGNITO", "cognito", "Amazon Cognito",
            "https://{domain}/oauth2/authorize", "https://{domain}/oauth2/token",
            Some("https://{domain}/oauth2/userinfo"),
            scopes ["openid", "profile", "email"],
            id ["sub", "username", "user_id"], name ["name", "given_name", "username"],
            email ["email"], image ["picture"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        prompt: Some("none"),
        image_fixup: ImageFixup::DiscordAvatar,
        ..standard!(
            "DISCORD", "discord", "Discord",
            "https://discord.com/api/oauth2/authorize", "https://discord.com/api/oauth2/token",
            Some("https://discord.com/api/users/@me"),
            scopes ["identify", "email"],
            id ["id"], name ["global_name", "username"], email ["email"], image ["avatar"], verified ["verified"]
        )
    },
    ProviderPreset {
        userinfo_method: Method::Post,
        ..standard!(
            "DROPBOX",
            "dropbox",
            "Dropbox",
            "https://www.dropbox.com/oauth2/authorize",
            "https://api.dropboxapi.com/oauth2/token",
            Some("https://api.dropboxapi.com/2/users/get_current_account"),
            scopes["account_info.read"],
            id["account_id"],
            name["name.display_name"],
            email["email"],
            image["profile_photo_url"],
            verified["email_verified"]
        )
    },
    ProviderPreset {
        auth_params: &[("display", "page")],
        userinfo_query: &[("fields", "id,name,email,picture")],
        ..standard!(
            "FACEBOOK", "facebook", "Facebook",
            "https://www.facebook.com/v24.0/dialog/oauth", "https://graph.facebook.com/v24.0/oauth/access_token",
            Some("https://graph.facebook.com/me"),
            scopes ["email", "public_profile"],
            id ["id", "user_id"], name ["name"], email ["email"], image ["picture.data.url", "picture"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        token_auth: TokenAuth::Basic,
        ..standard!(
            "FIGMA", "figma", "Figma",
            "https://www.figma.com/oauth", "https://api.figma.com/v1/oauth/token",
            Some("https://api.figma.com/v1/me"),
            scopes ["current_user:read"],
            id ["id"], name ["handle"], email ["email"], image ["img_url"], verified []
        )
    },
    ProviderPreset {
        url_vars: &[(
            "issuer",
            &["GITLAB_URL", "GITLAB_ISSUER"],
            Some("https://gitlab.com"),
        )],
        ..standard!(
            "GITLAB", "gitlab", "GitLab",
            "{issuer}/oauth/authorize", "{issuer}/oauth/token", Some("{issuer}/api/v4/user"),
            scopes ["read_user"],
            id ["id"], name ["name", "username"], email ["email"], image ["avatar_url"], verified ["email_verified"]
        )
    },
    standard!(
        "HUGGINGFACE", "huggingface", "Hugging Face",
        "https://huggingface.co/oauth/authorize", "https://huggingface.co/oauth/token",
        Some("https://huggingface.co/oauth/userinfo"),
        scopes ["openid", "profile", "email"],
        id ["sub"], name ["name", "preferred_username"], email ["email"], image ["picture"], verified ["email_verified"]
    ),
    standard!(
        "KAKAO", "kakao", "Kakao",
        "https://kauth.kakao.com/oauth/authorize", "https://kauth.kakao.com/oauth/token",
        Some("https://kapi.kakao.com/v2/user/me"),
        scopes ["account_email", "profile_image", "profile_nickname"],
        id ["id"], name ["kakao_account.profile.nickname"], email ["kakao_account.email"],
        image ["kakao_account.profile.thumbnail_image_url", "kakao_account.profile.profile_image_url"],
        verified ["kakao_account.is_email_verified"]
    ),
    standard!(
        "KICK", "kick", "Kick",
        "https://id.kick.com/oauth/authorize", "https://id.kick.com/oauth/token",
        Some("https://api.kick.com/public/v1/users"),
        scopes ["user:read"],
        id ["data.0.id"], name ["data.0.name"], email ["data.0.email"], image ["data.0.profile_picture"], verified []
    ),
    ProviderPreset {
        profile: ProfileSource::IdTokenOrUserinfo,
        ..standard!(
            "LINE", "line", "LINE",
            "https://access.line.me/oauth2/v2.1/authorize", "https://api.line.me/oauth2/v2.1/token",
            Some("https://api.line.me/oauth2/v2.1/userinfo"),
            scopes ["openid", "profile", "email"],
            id ["sub", "userId"], name ["name"], email ["email"], image ["picture", "pictureUrl"], verified []
        )
    },
    ProviderPreset {
        userinfo_method: Method::Post,
        userinfo_headers: &[("content-type", "application/json")],
        userinfo_body: Some(
            r#"{"query":"query { viewer { id name email avatarUrl active createdAt updatedAt } }"}"#,
        ),
        ..standard!(
            "LINEAR", "linear", "Linear",
            "https://linear.app/oauth/authorize", "https://api.linear.app/oauth/token",
            Some("https://api.linear.app/graphql"),
            scopes ["read"],
            id ["data.viewer.id"], name ["data.viewer.name"], email ["data.viewer.email"], image ["data.viewer.avatarUrl"], verified []
        )
    },
    standard!(
        "LINKEDIN", "linkedin", "LinkedIn",
        "https://www.linkedin.com/oauth/v2/authorization", "https://www.linkedin.com/oauth/v2/accessToken",
        Some("https://api.linkedin.com/v2/userinfo"),
        scopes ["profile", "email", "openid"],
        id ["sub"], name ["name"], email ["email"], image ["picture"], verified ["email_verified"]
    ),
    ProviderPreset {
        profile: ProfileSource::IdToken,
        url_vars: &[("tenant", &["MICROSOFT_TENANT_ID"], Some("common"))],
        ..standard!(
            "MICROSOFT", "microsoft", "Microsoft Entra ID",
            "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize",
            "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token", None,
            scopes ["openid", "profile", "email", "User.Read", "offline_access"],
            id ["oid", "sub"], name ["name"], email ["email", "preferred_username"], image ["picture"], verified ["email_verified"]
        )
    },
    // Cloudflare's OIDC userinfo endpoint carries only `sub`, so the
    // profile comes from the API's `/user` endpoint, which wraps it in a
    // `result` envelope.
    ProviderPreset {
        name_join: Some(("result.first_name", "result.last_name")),
        ..standard!(
            "CLOUDFLARE", "cloudflare", "Cloudflare",
            "https://dash.cloudflare.com/oauth2/auth", "https://dash.cloudflare.com/oauth2/token",
            Some("https://api.cloudflare.com/client/v4/user"),
            scopes ["user-details.read"],
            id ["result.id"], name ["result.email"], email ["result.email"], image [], verified []
        )
    },
    standard!(
        "NAVER", "naver", "Naver",
        "https://nid.naver.com/oauth2.0/authorize", "https://nid.naver.com/oauth2.0/token",
        Some("https://openapi.naver.com/v1/nid/me"),
        scopes ["profile", "email"],
        id ["response.id"], name ["response.name", "response.nickname"], email ["response.email"], image ["response.profile_image"], verified []
    ),
    // Notion returns the installing user inside the token response, and
    // wants the request as JSON. `/v1/users/me` is the fallback.
    ProviderPreset {
        profile: ProfileSource::TokenResponse,
        token_auth: TokenAuth::Basic,
        token_body: TokenBody::Json,
        auth_params: &[("owner", "user")],
        userinfo_headers: &[("notion-version", "2022-06-28")],
        ..standard!(
            "NOTION", "notion", "Notion",
            "https://api.notion.com/v1/oauth/authorize", "https://api.notion.com/v1/oauth/token",
            Some("https://api.notion.com/v1/users/me"),
            scopes [],
            id ["owner.user.id", "bot.owner.user.id"], name ["owner.user.name", "bot.owner.user.name"],
            email ["owner.user.person.email", "bot.owner.user.person.email"],
            image ["owner.user.avatar_url", "bot.owner.user.avatar_url"], verified []
        )
    },
    ProviderPreset {
        profile: ProfileSource::IdToken,
        url_vars: &[("issuer", &["PAYBIN_ISSUER"], Some("https://idp.paybin.io"))],
        ..standard!(
            "PAYBIN", "paybin", "Paybin",
            "{issuer}/oauth2/authorize", "{issuer}/oauth2/token", None,
            scopes ["openid", "email", "profile"],
            id ["sub"], name ["name", "preferred_username"], email ["email"], image ["picture"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        token_auth: TokenAuth::Basic,
        userinfo_query: &[("schema", "paypalv1.1")],
        url_vars: &[
            ("api_host", &["PAYPAL_API_HOST"], Some("api-m.paypal.com")),
            ("www_host", &["PAYPAL_WWW_HOST"], Some("www.paypal.com")),
        ],
        sandbox: Some((
            "PAYPAL_MODE",
            "api-m.sandbox.paypal.com",
            "www.sandbox.paypal.com",
        )),
        ..standard!(
            "PAYPAL", "paypal", "PayPal",
            "https://{www_host}/signin/authorize", "https://{api_host}/v1/oauth2/token",
            Some("https://{api_host}/v1/identity/oauth2/userinfo"),
            scopes ["openid", "email", "profile"],
            id ["user_id", "sub"], name ["name"], email ["email"], image ["picture"], verified ["email_verified"]
        )
    },
    standard!(
        "POLAR", "polar", "Polar",
        "https://polar.sh/oauth2/authorize", "https://api.polar.sh/v1/oauth2/token",
        Some("https://api.polar.sh/v1/oauth2/userinfo"),
        scopes ["openid", "profile", "email"],
        id ["id"], name ["public_name", "username"], email ["email"], image ["avatar_url"], verified ["email_verified"]
    ),
    ProviderPreset {
        token_auth: TokenAuth::Basic,
        userinfo_headers: &[("user-agent", "better-auth")],
        image_fixup: ImageFixup::StripQuery,
        ..standard!(
            "REDDIT", "reddit", "Reddit",
            "https://www.reddit.com/api/v1/authorize", "https://www.reddit.com/api/v1/access_token",
            Some("https://oauth.reddit.com/api/v1/me"),
            scopes ["identity"],
            id ["id"], name ["name"], email [], image ["icon_img"], verified []
        )
    },
    ProviderPreset {
        prompt: Some("select_account consent"),
        ..standard!(
            "ROBLOX", "roblox", "Roblox",
            "https://apis.roblox.com/oauth/v1/authorize", "https://apis.roblox.com/oauth/v1/token",
            Some("https://apis.roblox.com/oauth/v1/userinfo"),
            scopes ["openid", "profile"],
            id ["sub"], name ["nickname", "preferred_username"], email [], image ["picture"], verified []
        )
    },
    ProviderPreset {
        url_vars: &[(
            "login_url",
            &["SALESFORCE_LOGIN_URL", "SALESFORCE_DOMAIN"],
            Some("https://login.salesforce.com"),
        )],
        ..standard!(
            "SALESFORCE", "salesforce", "Salesforce",
            "{login_url}/services/oauth2/authorize", "{login_url}/services/oauth2/token",
            Some("{login_url}/services/oauth2/userinfo"),
            scopes ["openid", "email", "profile"],
            id ["user_id"], name ["name"], email ["email"], image ["photos.picture", "photos.thumbnail"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        ..standard!(
            "SLACK", "slack", "Slack",
            "https://slack.com/openid/connect/authorize", "https://slack.com/api/openid.connect.token",
            Some("https://slack.com/api/openid.connect.userInfo"),
            scopes ["openid", "profile", "email"],
            id ["sub", "https://slack.com/user_id"], name ["name", "https://slack.com/name"], email ["email"],
            image ["picture", "https://slack.com/user_image_512"], verified ["email_verified"]
        )
    },
    standard!(
        "RAILWAY", "railway", "Railway",
        "https://backboard.railway.com/oauth/auth", "https://backboard.railway.com/oauth/token",
        Some("https://backboard.railway.com/oauth/me"),
        scopes ["openid", "email", "profile"],
        id ["sub"], name ["name"], email ["email"], image ["picture"], verified ["email_verified"]
    ),
    standard!(
        "SPOTIFY", "spotify", "Spotify",
        "https://accounts.spotify.com/authorize", "https://accounts.spotify.com/api/token",
        Some("https://api.spotify.com/v1/me"),
        scopes ["user-read-email"],
        id ["id"], name ["display_name"], email ["email"], image ["images.0.url"], verified []
    ),
    // Vercel asks for no scopes of its own, and the profile may have no
    // name but a preferred username.
    ProviderPreset {
        scopes: &[],
        ..standard!(
            "VERCEL", "vercel", "Vercel",
            "https://vercel.com/oauth/authorize", "https://api.vercel.com/login/oauth/token",
            Some("https://api.vercel.com/login/oauth/userinfo"),
            scopes [],
            id ["sub"], name ["name", "preferred_username"], email ["email"], image ["picture"],
            verified ["email_verified"]
        )
    },
    // TikTok names the client `client_key` everywhere.
    ProviderPreset {
        client_id_param: "client_key",
        userinfo_query: &[("fields", "open_id,avatar_large_url,display_name,username")],
        ..standard!(
            "TIKTOK", "tiktok", "TikTok",
            "https://www.tiktok.com/v2/auth/authorize/", "https://open.tiktokapis.com/v2/oauth/token/",
            Some("https://open.tiktokapis.com/v2/user/info/"),
            scopes ["user.info.profile"],
            id ["data.user.open_id"], name ["data.user.display_name", "data.user.username"], email ["data.user.email"],
            image ["data.user.avatar_large_url"], verified []
        )
    },
    // WeChat is not OAuth2 in the usual shape: the client is `appid`, the
    // secret is `secret`, the token is fetched with a GET, the
    // authorization URL ends in `#wechat_redirect`, and the userinfo
    // endpoint needs the `openid` that came back with the token.
    ProviderPreset {
        client_id_param: "appid",
        client_secret_param: "secret",
        token_method: Method::Get,
        token_omits: &["redirect_uri", "code_verifier"],
        auth_url_fragment: Some("wechat_redirect"),
        userinfo_query_from_token: &["openid"],
        userinfo_query: &[("lang", "zh_CN")],
        userinfo_auth: UserinfoAuth::Query,
        scopes: &["snsapi_login"],
        scope_separator: ",",
        auth_params: &[("lang", "cn")],
        pkce: false,
        id_paths: &["unionid", "openid"],
        name_paths: &["nickname"],
        email_paths: &["email"],
        image_paths: &["headimgurl"],
        ..standard!(
            "WECHAT", "wechat", "WeChat",
            "https://open.weixin.qq.com/connect/qrconnect",
            "https://api.weixin.qq.com/sns/oauth2/access_token",
            Some("https://api.weixin.qq.com/sns/userinfo"),
            scopes ["snsapi_login"],
            id ["unionid", "openid"], name ["nickname"], email ["email"], image ["headimgurl"], verified []
        )
    },
    ProviderPreset {
        profile: ProfileSource::IdTokenOrUserinfo,
        userinfo_client_id_header: Some("Client-Id"),
        ..standard!(
            "TWITCH", "twitch", "Twitch",
            "https://id.twitch.tv/oauth2/authorize", "https://id.twitch.tv/oauth2/token",
            Some("https://api.twitch.tv/helix/users"),
            scopes ["user:read:email", "openid"],
            id ["sub", "data.0.id"], name ["preferred_username", "data.0.display_name", "data.0.login"],
            email ["email", "data.0.email"], image ["picture", "data.0.profile_image_url"], verified ["email_verified"]
        )
    },
    ProviderPreset {
        token_auth: TokenAuth::Basic,
        email_endpoint: Some(EmailEndpoint {
            url: "https://api.x.com/2/users/me?user.fields=confirmed_email",
            primary_array: false,
            path: "data.confirmed_email",
            verified_path: None,
        }),
        ..standard!(
            "TWITTER", "twitter", "X (Twitter)",
            "https://x.com/i/oauth2/authorize", "https://api.x.com/2/oauth2/token",
            Some("https://api.x.com/2/users/me?user.fields=profile_image_url"),
            scopes ["users.read", "tweet.read", "offline.access", "users.email"],
            id ["data.id"], name ["data.name"], email ["data.email"], image ["data.profile_image_url"], verified []
        )
    },
    // VK's userinfo is a form POST with the token in the body, and the
    // email rides on the token response.
    ProviderPreset {
        userinfo_method: Method::Post,
        userinfo_auth: UserinfoAuth::FormBody,
        name_join: Some(("user.first_name", "user.last_name")),
        token_email_path: Some("email"),
        ..standard!(
            "VK", "vk", "VK",
            "https://id.vk.com/authorize", "https://id.vk.com/oauth2/auth",
            Some("https://id.vk.com/oauth2/user_info"),
            scopes ["email", "phone"],
            id ["user.id", "user_id"], name [], email ["user.email"], image ["user.avatar"], verified []
        )
    },
    ProviderPreset {
        ..standard!(
            "ZOOM", "zoom", "Zoom",
            "https://zoom.us/oauth/authorize", "https://zoom.us/oauth/token",
            Some("https://api.zoom.us/v2/users/me"),
            scopes ["user:read", "user_info:read"],
            id ["id"], name ["display_name"], email ["email"], image ["pic_url"], verified ["verified"]
        )
    },
];

/// The preset with this id, if it is one of the built-ins.
pub fn preset(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// A configured provider: a preset (or custom definition) with credentials
/// and resolved URLs. This is what the routes use.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderConfig {
    pub id: String,
    pub display_name: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub userinfo_method: Method,
    pub userinfo_auth: UserinfoAuth,
    pub userinfo_headers: Vec<(String, String)>,
    pub userinfo_client_id_header: Option<String>,
    pub userinfo_body: Option<String>,
    pub userinfo_query: Vec<(String, String)>,
    pub scopes: Vec<String>,
    /// What joins several scopes.
    pub scope_separator: String,
    pub pkce: bool,
    pub token_auth: TokenAuth,
    pub token_body: TokenBody,
    pub client_id_param: String,
    /// The token request's secret parameter.
    pub client_secret_param: String,
    /// How the token request is sent.
    pub token_method: Method,
    /// Extra fields in the token request.
    pub token_extra_fields: Vec<(String, String)>,
    /// Standard fields the token request leaves out.
    pub token_omits: Vec<String>,
    /// A fragment on the authorization URL.
    pub auth_url_fragment: Option<String>,
    /// Userinfo query parameters taken from the token response.
    pub userinfo_query_from_token: Vec<String>,
    pub response_type: String,
    pub response_mode: Option<String>,
    pub prompt: Option<String>,
    pub auth_params: Vec<(String, String)>,
    pub profile: ProfileSource,
    pub id_paths: Vec<String>,
    pub name_paths: Vec<String>,
    pub name_join: Option<(String, String)>,
    pub email_paths: Vec<String>,
    pub image_paths: Vec<String>,
    pub image_fixup: ImageFixup,
    pub verified_paths: Vec<String>,
    pub email_endpoint: Option<EmailEndpointOwned>,
    pub token_email_path: Option<String>,
    /// Where the provider publishes its OpenID configuration.
    ///
    /// When this is set, the endpoints below are read from the document it
    /// publishes, and an ID token is checked against the key set it names.
    pub discovery_url: Option<String>,
    /// The provider's key set, once discovery has been read.
    pub jwks_uri: Option<String>,
    /// The issuer an ID token has to name.
    pub issuer: Option<String>,
    /// Whether an ID token that cannot be checked is a failure.
    pub require_id_token_verification: bool,
    /// Whether the `nonce` claim is left out of the ID-token check.
    pub disable_id_token_nonce_binding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailEndpointOwned {
    pub url: String,
    pub primary_array: bool,
    pub path: String,
    pub verified_path: Option<String>,
}

impl EmailEndpointOwned {
    fn from_preset(e: &EmailEndpoint) -> Self {
        EmailEndpointOwned {
            url: e.url.to_string(),
            primary_array: e.primary_array,
            path: e.path.to_string(),
            verified_path: e.verified_path.map(str::to_string),
        }
    }
}

/// A provider as described by an embedder that needs one we do not ship.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomProvider {
    pub id: String,
    pub display_name: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub scopes: Vec<String>,
    pub pkce: bool,
    pub token_auth: Option<TokenAuth>,
    /// Dot-paths into the userinfo document.
    pub id_path: Option<String>,
    pub name_path: Option<String>,
    pub email_path: Option<String>,
    pub image_path: Option<String>,
    pub verified_path: Option<String>,
    /// Where the provider publishes its OpenID configuration.
    pub discovery_url: Option<String>,
    /// The issuer an ID token has to name. Read from discovery when it is
    /// left out.
    pub issuer: Option<String>,
    /// Whether an ID token that cannot be checked is a failure.
    pub require_id_token_verification: bool,
    /// Whether the `nonce` claim is left out of the ID-token check.
    pub disable_id_token_nonce_binding: bool,
    /// How the token request is sent.
    pub token_method: Option<Method>,
    /// The wire format of the token request body.
    pub token_body: Option<TokenBody>,
    /// The token request's client-id parameter.
    pub client_id_param: Option<String>,
    /// The token request's secret parameter.
    pub client_secret_param: Option<String>,
    /// Extra fields in the token request.
    pub token_extra_fields: Vec<(String, String)>,
    /// How the userinfo endpoint is called.
    pub userinfo_method: Option<Method>,
    /// How the userinfo endpoint is authenticated.
    pub userinfo_auth: Option<UserinfoAuth>,
    /// What joins several scopes.
    pub scope_separator: Option<String>,
    /// The response type of the authorization request.
    pub response_type: Option<String>,
    /// The prompt of the authorization request.
    pub prompt: Option<String>,
}

/// The paths a provider uses for one field: the one it named, or the
/// usual ones.
fn paths(named: &Option<String>, usual: &[&str]) -> Vec<String> {
    match named {
        Some(path) => vec![path.clone()],
        None => usual.iter().map(|p| (*p).to_string()).collect(),
    }
}

/// Why a provider could not be configured from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `CLIENT_ID` or `CLIENT_SECRET` for the provider is missing.
    MissingCredential { provider: String, variable: String },
    /// A URL placeholder has no variable and no default.
    MissingVariable {
        provider: String,
        placeholder: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingCredential { provider, variable } => {
                write!(f, "{provider}: {variable} is not set")
            }
            ConfigError::MissingVariable {
                provider,
                placeholder,
            } => write!(f, "{provider}: no value for {{{placeholder}}}"),
        }
    }
}

/// Fills `{placeholders}` in a URL. Returns `None` when one has no value.
fn expand(template: &str, vars: &BTreeMap<String, String>) -> Option<String> {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    if out.contains('{') && out.contains('}') {
        return None;
    }
    Some(out)
}

impl ProviderConfig {
    /// Configure a preset with explicit credentials and URL variables.
    pub fn from_preset(
        preset: &ProviderPreset,
        client_id: &str,
        client_secret: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<ProviderConfig, ConfigError> {
        let mut vars = vars.clone();
        // Sandbox hosts first, so the live defaults below do not win.
        if let Some((_, api, www)) = preset.sandbox {
            if vars.get("sandbox").is_some_and(|v| v == "sandbox") {
                vars.entry("api_host".into())
                    .or_insert_with(|| api.to_string());
                vars.entry("www_host".into())
                    .or_insert_with(|| www.to_string());
            }
        }
        for (placeholder, _, default) in preset.url_vars {
            if let Some(default) = default {
                vars.entry((*placeholder).to_string())
                    .or_insert_with(|| (*default).to_string());
            }
        }

        let missing = |placeholder: &str| ConfigError::MissingVariable {
            provider: preset.id.to_string(),
            placeholder: placeholder.to_string(),
        };
        let authorization_url = expand(preset.authorization_url, &vars)
            .filter(|u| !u.contains('{'))
            .ok_or_else(|| missing(first_placeholder(preset.authorization_url)))?;
        let token_url = expand(preset.token_url, &vars)
            .filter(|u| !u.contains('{'))
            .ok_or_else(|| missing(first_placeholder(preset.token_url)))?;
        let userinfo_url = match preset.userinfo_url {
            Some(template) => Some(
                expand(template, &vars)
                    .filter(|u| !u.contains('{'))
                    .ok_or_else(|| missing(first_placeholder(template)))?,
            ),
            None => None,
        };

        Ok(ProviderConfig {
            id: preset.id.to_string(),
            display_name: preset.display_name.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            authorization_url,
            token_url,
            userinfo_url,
            userinfo_method: preset.userinfo_method,
            userinfo_auth: preset.userinfo_auth,
            userinfo_headers: preset
                .userinfo_headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            userinfo_client_id_header: preset.userinfo_client_id_header.map(str::to_string),
            userinfo_body: preset.userinfo_body.map(str::to_string),
            userinfo_query: preset
                .userinfo_query
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            scopes: preset.scopes.iter().map(|s| s.to_string()).collect(),
            scope_separator: preset.scope_separator.to_string(),
            pkce: preset.pkce,
            token_auth: preset.token_auth,
            token_body: preset.token_body,
            client_id_param: preset.client_id_param.to_string(),
            client_secret_param: preset.client_secret_param.to_string(),
            token_method: preset.token_method,
            token_extra_fields: preset
                .token_extra_fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            token_omits: preset.token_omits.iter().map(|s| s.to_string()).collect(),
            auth_url_fragment: preset.auth_url_fragment.map(str::to_string),
            userinfo_query_from_token: preset
                .userinfo_query_from_token
                .iter()
                .map(|s| s.to_string())
                .collect(),
            response_type: preset.response_type.to_string(),
            response_mode: preset.response_mode.map(str::to_string),
            prompt: preset.prompt.map(str::to_string),
            auth_params: preset
                .auth_params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            profile: preset.profile,
            id_paths: preset.id_paths.iter().map(|s| s.to_string()).collect(),
            name_paths: preset.name_paths.iter().map(|s| s.to_string()).collect(),
            name_join: preset
                .name_join
                .map(|(a, b)| (a.to_string(), b.to_string())),
            email_paths: preset.email_paths.iter().map(|s| s.to_string()).collect(),
            image_paths: preset.image_paths.iter().map(|s| s.to_string()).collect(),
            image_fixup: preset.image_fixup,
            verified_paths: preset
                .verified_paths
                .iter()
                .map(|s| s.to_string())
                .collect(),
            email_endpoint: preset
                .email_endpoint
                .as_ref()
                .map(EmailEndpointOwned::from_preset),
            token_email_path: preset.token_email_path.map(str::to_string),
            discovery_url: None,
            jwks_uri: None,
            issuer: None,
            require_id_token_verification: false,
            disable_id_token_nonce_binding: false,
        })
    }

    /// Configure a provider the embedder described in code.
    pub fn from_custom(c: &CustomProvider) -> ProviderConfig {
        ProviderConfig {
            id: c.id.clone(),
            display_name: if c.display_name.is_empty() {
                c.id.clone()
            } else {
                c.display_name.clone()
            },
            client_id: c.client_id.clone(),
            client_secret: c.client_secret.clone(),
            authorization_url: c.authorization_url.clone(),
            token_url: c.token_url.clone(),
            userinfo_url: c.userinfo_url.clone(),
            userinfo_method: c.userinfo_method.unwrap_or(Method::Get),
            userinfo_auth: c.userinfo_auth.unwrap_or(UserinfoAuth::Bearer),
            userinfo_headers: Vec::new(),
            userinfo_client_id_header: None,
            userinfo_body: None,
            userinfo_query: Vec::new(),
            scopes: c.scopes.clone(),
            scope_separator: c.scope_separator.clone().unwrap_or_else(|| " ".into()),
            pkce: c.pkce,
            token_auth: c.token_auth.unwrap_or(TokenAuth::Body),
            token_body: c.token_body.unwrap_or(TokenBody::Form),
            client_id_param: "client_id".into(),
            client_secret_param: "client_secret".into(),
            token_method: Method::Post,
            token_extra_fields: Vec::new(),
            token_omits: Vec::new(),
            auth_url_fragment: None,
            userinfo_query_from_token: Vec::new(),
            response_type: c.response_type.clone().unwrap_or_else(|| "code".into()),
            response_mode: None,
            prompt: c.prompt.clone(),
            auth_params: Vec::new(),
            profile: if c.userinfo_url.is_some() {
                ProfileSource::Userinfo
            } else {
                ProfileSource::IdToken
            },
            // A provider that says nothing about its fields gets the
            // usual ones: the subject for the account, the name, the
            // address, the picture and whether the address is proven.
            id_paths: paths(&c.id_path, &["sub", "id"]),
            name_paths: paths(&c.name_path, &["name"]),
            name_join: None,
            email_paths: paths(&c.email_path, &["email"]),
            image_paths: paths(&c.image_path, &["picture", "image", "avatar_url"]),
            image_fixup: ImageFixup::None,
            verified_paths: paths(&c.verified_path, &["email_verified"]),
            discovery_url: c.discovery_url.clone(),
            jwks_uri: None,
            issuer: c.issuer.clone(),
            require_id_token_verification: c.require_id_token_verification,
            disable_id_token_nonce_binding: c.disable_id_token_nonce_binding,
            email_endpoint: None,
            token_email_path: None,
        }
    }

    /// The authorization request the browser is redirected to.
    pub fn authorization_url(
        &self,
        redirect_uri: &str,
        state: &str,
        code_challenge: Option<&str>,
        scopes: &[String],
        login_hint: Option<&str>,
        prompt: Option<&str>,
    ) -> String {
        self.authorization_url_with(
            redirect_uri,
            state,
            code_challenge,
            scopes,
            login_hint,
            prompt,
            &[],
        )
    }

    /// The authorization URL, with extra parameters for this one request.
    #[allow(clippy::too_many_arguments)]
    ///
    /// The extras are for things that belong to a single sign-in — the
    /// `nonce` that binds an ID token to it — rather than to the provider.
    pub fn authorization_url_with(
        &self,
        redirect_uri: &str,
        state: &str,
        code_challenge: Option<&str>,
        scopes: &[String],
        login_hint: Option<&str>,
        prompt: Option<&str>,
        extra: &[(&str, &str)],
    ) -> String {
        let mut params: Vec<(String, String)> = vec![
            ("response_type".into(), self.response_type.clone()),
            (self.client_id_param.clone(), self.client_id.clone()),
            ("redirect_uri".into(), redirect_uri.to_string()),
            ("state".into(), state.to_string()),
        ];
        let scopes: Vec<String> = if scopes.is_empty() {
            self.scopes.clone()
        } else {
            scopes.to_vec()
        };
        if !scopes.is_empty() {
            params.push(("scope".into(), scopes.join(&self.scope_separator)));
        }
        if let Some(mode) = &self.response_mode {
            params.push(("response_mode".into(), mode.clone()));
        }
        if let Some(challenge) = code_challenge {
            params.push(("code_challenge".into(), challenge.to_string()));
            params.push(("code_challenge_method".into(), "S256".into()));
        }
        if let Some(hint) = login_hint.filter(|h| !h.is_empty()) {
            params.push(("login_hint".into(), hint.to_string()));
        }
        if let Some(prompt) = prompt.filter(|p| !p.is_empty()).or(self.prompt.as_deref()) {
            params.push(("prompt".into(), prompt.to_string()));
        }
        for (k, v) in &self.auth_params {
            params.push((k.clone(), v.clone()));
        }
        for (k, v) in extra {
            params.push(((*k).to_string(), (*v).to_string()));
        }
        let query: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect();
        let separator = if self.authorization_url.contains('?') {
            "&".to_string()
        } else {
            "?".to_string()
        };
        let fragment = match &self.auth_url_fragment {
            Some(fragment) => format!("#{fragment}"),
            None => String::new(),
        };
        format!(
            "{}{}{}{}",
            self.authorization_url,
            separator,
            query.join("&"),
            fragment
        )
    }

    /// The token request for an authorization code: (content-type, body
    /// bytes, headers).
    pub fn token_request(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: Option<&str>,
    ) -> (String, Vec<u8>, Vec<(String, String)>) {
        let mut fields: Vec<(String, String)> = vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code.to_string()),
            ("redirect_uri".into(), redirect_uri.to_string()),
        ];
        if let Some(verifier) = code_verifier {
            fields.push(("code_verifier".into(), verifier.to_string()));
        }
        let mut headers: Vec<(String, String)> = vec![("accept".into(), "application/json".into())];
        match self.token_auth {
            TokenAuth::Basic => {
                let raw = format!("{}:{}", self.client_id, self.client_secret);
                headers.push((
                    "authorization".into(),
                    format!("Basic {}", b64(raw.as_bytes())),
                ));
            }
            TokenAuth::Body => {
                // The client parameter keeps the provider's name: TikTok
                // calls it `client_key`, WeChat `appid`.
                fields.push((self.client_id_param.clone(), self.client_id.clone()));
                fields.push((self.client_secret_param.clone(), self.client_secret.clone()));
            }
        }
        // A provider that wants neither `redirect_uri` nor `code_verifier`
        // says so, and the extra fields it wants are added here.
        fields.retain(|(name, _)| !self.token_omits.iter().any(|o| o == name));
        for (name, value) in &self.token_extra_fields {
            fields.push((name.clone(), value.clone()));
        }
        if self.token_method == Method::Get {
            // The fields go in the query string, as WeChat wants.
            let query: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                .collect();
            let separator = if self.token_url.contains('?') {
                "&"
            } else {
                "?"
            };
            return (
                String::new(),
                format!("{separator}{}", query.join("&")).into_bytes(),
                headers,
            );
        }
        match self.token_body {
            TokenBody::Form => {
                headers.push((
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                ));
                (
                    String::new(),
                    form_encode(
                        &fields
                            .iter()
                            .map(|(a, b)| (a.as_str(), b.as_str()))
                            .collect::<Vec<_>>(),
                    )
                    .into_bytes(),
                    headers,
                )
            }
            TokenBody::Json => {
                headers.push(("content-type".into(), "application/json".into()));
                let mut o = mosql::Object::new();
                for (k, v) in &fields {
                    o.set(k.as_str(), Value::str(v.clone()));
                }
                (
                    String::new(),
                    mosql::to_json(&Value::Object(o)).into_bytes(),
                    headers,
                )
            }
        }
    }

    /// The refresh request for a stored refresh token.
    pub fn refresh_request(&self, refresh_token: &str) -> (Vec<u8>, Vec<(String, String)>) {
        let mut fields: Vec<(String, String)> = vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh_token.to_string()),
        ];
        if !self.scopes.is_empty() {
            fields.push(("scope".into(), self.scopes.join(" ")));
        }
        let mut headers: Vec<(String, String)> = vec![
            ("accept".into(), "application/json".into()),
            (
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            ),
        ];
        match self.token_auth {
            TokenAuth::Basic => {
                let raw = format!("{}:{}", self.client_id, self.client_secret);
                headers.push((
                    "authorization".into(),
                    format!("Basic {}", b64(raw.as_bytes())),
                ));
            }
            TokenAuth::Body => {
                // The client parameter keeps the provider's name: TikTok
                // calls it `client_key`.
                fields.push((self.client_id_param.clone(), self.client_id.clone()));
                fields.push(("client_secret".into(), self.client_secret.clone()));
            }
        }
        (
            form_encode(
                &fields
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect::<Vec<_>>(),
            )
            .into_bytes(),
            headers,
        )
    }
}

fn first_placeholder(template: &str) -> &str {
    let start = template.find('{').map(|i| i + 1).unwrap_or(0);
    let end = template[start..]
        .find('}')
        .map(|i| i + start)
        .unwrap_or(template.len());
    template[start..end].trim()
}

/// Standard base64 (RFC 4648 with padding), as HTTP Basic needs.
fn b64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ------------------------------------------------------------- the registry ---

/// Every provider this server can sign in with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OAuthRegistry {
    providers: BTreeMap<String, ProviderConfig>,
}

impl OAuthRegistry {
    pub fn new() -> Self {
        OAuthRegistry::default()
    }

    /// Read every preset's credentials from the environment. A provider
    /// whose variables are absent is simply not offered; one whose
    /// credentials are half set is reported in `problems` so the operator
    /// sees the typo.
    pub fn from_env() -> (OAuthRegistry, Vec<ConfigError>) {
        let lookup = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let (mut registry, problems) = Self::from_env_with(lookup);
        let names = |prefix: &str| {
            std::env::vars()
                .map(|(key, _)| key)
                .filter(|key| key.starts_with(prefix))
                .collect::<Vec<String>>()
        };
        let (generic, generic_problems) = Self::from_env_generic(lookup, names);
        for config in generic {
            registry.insert(config);
        }
        let problems = problems.into_iter().chain(generic_problems).collect();
        (registry, problems)
    }

    /// [`OAuthRegistry::from_env`] with the variable lookup supplied, for
    /// tests and embedders with their own configuration source.
    pub fn from_env_with(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> (OAuthRegistry, Vec<ConfigError>) {
        let mut registry = OAuthRegistry::new();
        let mut problems = Vec::new();
        for preset in PRESETS.iter() {
            let client_id = lookup(preset.client_id_env);
            let client_secret = lookup(preset.client_secret_env);
            match (client_id, client_secret) {
                (Some(id), Some(secret)) => {
                    let mut vars = BTreeMap::new();
                    // Only what the environment says: defaults are applied
                    // by `from_preset`, after the sandbox hosts.
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
                    match ProviderConfig::from_preset(preset, &id, &secret, &vars) {
                        Ok(config) => {
                            registry.providers.insert(config.id.clone(), config);
                        }
                        Err(e) => problems.push(e),
                    }
                }
                (Some(_), None) => problems.push(ConfigError::MissingCredential {
                    provider: preset.id.to_string(),
                    variable: preset.client_secret_env.to_string(),
                }),
                (None, Some(_)) => problems.push(ConfigError::MissingCredential {
                    provider: preset.id.to_string(),
                    variable: preset.client_id_env.to_string(),
                }),
                (None, None) => {}
            }
        }
        (registry, problems)
    }

    /// Add or replace a provider described in code.
    pub fn insert(&mut self, config: ProviderConfig) {
        self.providers.insert(config.id.clone(), config);
    }

    /// The providers the environment describes, besides the presets.
    ///
    /// A provider that is not one of ours is described by its variables:
    ///
    /// ```text
    /// OAUTH_ACME_CLIENT_ID=…
    /// OAUTH_ACME_CLIENT_SECRET=…
    /// OAUTH_ACME_DISCOVERY_URL=https://acme.test/.well-known/openid-configuration
    /// ```
    ///
    /// The name after `OAUTH_` is the provider's id. The discovery URL
    /// says where its endpoints are; without one, the endpoints are named
    /// outright.
    pub fn from_env_generic(
        lookup: impl Fn(&str) -> Option<String>,
        names: impl Fn(&str) -> Vec<String>,
    ) -> (Vec<ProviderConfig>, Vec<ConfigError>) {
        let mut providers = Vec::new();
        let mut problems = Vec::new();
        // A provider is named by its client-id or client-secret variable:
        // `OAUTH_ACME_CLIENT_ID` or `OAUTH_ACME_CLIENT_SECRET`. Both are
        // looked for, so a provider that is only half described is still
        // seen — and can be reported.
        let mut seen: Vec<String> = Vec::new();
        for name in names("OAUTH_") {
            let Some(rest) = name.strip_prefix("OAUTH_") else {
                continue;
            };
            let Some(id) = rest
                .strip_suffix("_CLIENT_ID")
                .or_else(|| rest.strip_suffix("_CLIENT_SECRET"))
            else {
                continue;
            };
            if id.is_empty() || seen.iter().any(|s| s == id) {
                continue;
            }
            seen.push(id.to_string());
            let id = id.to_ascii_lowercase();
            let client_id = lookup(&format!("OAUTH_{}_CLIENT_ID", id.to_uppercase()));
            let client_secret = lookup(&format!("OAUTH_{}_CLIENT_SECRET", id.to_uppercase()));
            let (Some(client_id), Some(client_secret)) = (client_id.clone(), client_secret.clone())
            else {
                // Half a provider is a mistake worth reporting, and the
                // report names the variable that is missing.
                if client_id.is_some() || client_secret.is_some() {
                    let missing = if client_id.is_none() {
                        "CLIENT_ID"
                    } else {
                        "CLIENT_SECRET"
                    };
                    problems.push(ConfigError::MissingCredential {
                        provider: id.clone(),
                        variable: format!("OAUTH_{}_{missing}", id.to_uppercase()),
                    });
                }
                continue;
            };
            let discovery_url = lookup(&format!("OAUTH_{}_DISCOVERY_URL", id.to_uppercase()));
            let authorization_url =
                lookup(&format!("OAUTH_{}_AUTHORIZATION_URL", id.to_uppercase()));
            let token_url = lookup(&format!("OAUTH_{}_TOKEN_URL", id.to_uppercase()));
            let userinfo_url = lookup(&format!("OAUTH_{}_USERINFO_URL", id.to_uppercase()));
            if discovery_url.is_none() && (authorization_url.is_none() || token_url.is_none()) {
                problems.push(ConfigError::MissingVariable {
                    provider: id.clone(),
                    placeholder: "DISCOVERY_URL".into(),
                });
                continue;
            }
            let scopes = lookup(&format!("OAUTH_{}_SCOPES", id.to_uppercase()))
                .map(|s| {
                    s.split([',', ' '])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_else(|| {
                    vec![
                        "openid".to_string(),
                        "profile".to_string(),
                        "email".to_string(),
                    ]
                });
            let flag = |name: &str| {
                lookup(&format!("OAUTH_{}_{}", id.to_uppercase(), name))
                    .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(false)
            };
            let custom = CustomProvider {
                id: id.clone(),
                display_name: lookup(&format!("OAUTH_{}_NAME", id.to_uppercase()))
                    .unwrap_or_else(|| id.clone()),
                client_id,
                client_secret,
                authorization_url: authorization_url.unwrap_or_default(),
                token_url: token_url.unwrap_or_default(),
                userinfo_url,
                scopes,
                pkce: !flag("NO_PKCE"),
                token_auth: None,
                id_path: lookup(&format!("OAUTH_{}_ID_PATH", id.to_uppercase())),
                name_path: lookup(&format!("OAUTH_{}_NAME_PATH", id.to_uppercase())),
                email_path: lookup(&format!("OAUTH_{}_EMAIL_PATH", id.to_uppercase())),
                image_path: lookup(&format!("OAUTH_{}_IMAGE_PATH", id.to_uppercase())),
                verified_path: lookup(&format!("OAUTH_{}_VERIFIED_PATH", id.to_uppercase())),
                discovery_url,
                issuer: lookup(&format!("OAUTH_{}_ISSUER", id.to_uppercase())),
                require_id_token_verification: flag("REQUIRE_ID_TOKEN_VERIFICATION"),
                disable_id_token_nonce_binding: flag("DISABLE_ID_TOKEN_NONCE_BINDING"),
                token_method: None,
                token_body: None,
                client_id_param: None,
                client_secret_param: None,
                token_extra_fields: Vec::new(),
                userinfo_method: None,
                userinfo_auth: None,
                scope_separator: None,
                response_type: None,
                prompt: None,
            };
            providers.push(ProviderConfig::from_custom(&custom));
        }
        (providers, problems)
    }

    pub fn get(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.get(id)
    }

    /// Ids in alphabetical order.
    pub fn ids(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

// ---------------------------------------------------------- profile reading ---

/// A userinfo request, ready for an `HttpClient`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserinfoRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ProviderConfig {
    /// Build the profile request for an access token.
    pub fn userinfo_request(&self, access_token: &str, token: &Value) -> Option<UserinfoRequest> {
        let url = self.userinfo_url.as_ref()?;
        let mut url = url.clone();
        let mut query: Vec<String> = self
            .userinfo_query
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect();
        // Some providers need a value that only the token response carried:
        // WeChat's userinfo endpoint wants the `openid` that came with the
        // access token.
        for name in &self.userinfo_query_from_token {
            if let Some(value) = token.get(name).and_then(Value::as_str) {
                query.push(format!(
                    "{}={}",
                    percent_encode(name),
                    percent_encode(value)
                ));
            }
        }
        let mut headers: Vec<(String, String)> = self.userinfo_headers.clone();
        headers.push(("accept".into(), "application/json".into()));
        let mut body = Vec::new();
        match self.userinfo_auth {
            UserinfoAuth::Bearer => {
                headers.push(("authorization".into(), format!("Bearer {access_token}")));
                if let Some(name) = &self.userinfo_client_id_header {
                    headers.push((name.clone(), self.client_id.clone()));
                }
            }
            UserinfoAuth::FormBody => {
                headers.push((
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                ));
                body = form_encode(&[
                    ("access_token", access_token),
                    ("client_id", &self.client_id),
                ])
                .into_bytes();
            }
            UserinfoAuth::Query => {
                // The token rides in the query string, not in a header.
                query.push(format!("access_token={}", percent_encode(access_token)));
            }
        }
        if let Some(fixed) = &self.userinfo_body {
            body = fixed.clone().into_bytes();
        }
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query.join("&"));
        }
        Some(UserinfoRequest {
            method: self.userinfo_method,
            url,
            headers,
            body,
        })
    }
}

/// What the provider told us about the person who signed in.
#[derive(Debug, Clone, PartialEq)]
pub struct OAuthProfile {
    /// The provider's stable identifier for the account.
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub image: Option<String>,
    pub email_verified: bool,
    /// The raw document the fields were read from, for the account row.
    pub raw: Value,
}

/// Discord returns an avatar hash, or null for the default; the picture
/// URL is assembled from the hash and the account id.
fn discord_avatar(source: &Value, paths: &[String], id: &str) -> String {
    match first_string(source, paths) {
        Some(avatar) => {
            let ext = if avatar.starts_with("a_") {
                "gif"
            } else {
                "png"
            };
            format!("https://cdn.discordapp.com/avatars/{id}/{avatar}.{ext}")
        }
        None => format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            (id.parse::<u64>().unwrap_or(0) >> 22) % 6
        ),
    }
}

/// Read a boolean the way providers spell them: `true`, `"true"`, `1`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Str(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes"),
        _ => false,
    }
}

/// A profile field as text. Providers are inconsistent about whether an
/// id is a JSON string or a JSON number.
fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.clone()),
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

fn first_string(value: &Value, paths: &[String]) -> Option<String> {
    paths
        .iter()
        .filter_map(|p| lookup(value, p).and_then(as_text))
        .find(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
}

fn string_at(value: &Value, path: &str) -> Option<String> {
    lookup(value, path)
        .and_then(as_text)
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
}

/// Apply the preset's dot-paths to the documents the provider returned.
///
/// * `token` is the token endpoint's JSON (Notion's inline profile, VK's
///   email).
/// * `profile` is the userinfo document or the decoded id token.
/// * `extra_email` is the optional second email document (GitHub's emails,
///   Twitter's confirmed address) and its verified flag.
pub fn extract_profile(
    provider: &ProviderConfig,
    token: &Value,
    profile: &Value,
    email_lookup: Option<(&Value, bool)>,
) -> OAuthProfile {
    let source = match provider.profile {
        ProfileSource::TokenResponse => token,
        _ => profile,
    };
    let id = first_string(source, &provider.id_paths).unwrap_or_default();
    let name = if let Some((a, b)) = &provider.name_join {
        let joined = [lookup(source, a), lookup(source, b)]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if joined.trim().is_empty() {
            first_string(source, &provider.name_paths)
        } else {
            Some(joined)
        }
        .unwrap_or_default()
    } else {
        first_string(source, &provider.name_paths).unwrap_or_default()
    };

    let mut email = first_string(source, &provider.email_paths);
    if email.is_none() {
        if let Some(path) = &provider.token_email_path {
            email = string_at(token, path);
        }
    }
    let mut email_verified = provider
        .verified_paths
        .iter()
        .any(|p| lookup(source, p).is_some_and(truthy));
    if email.is_none() {
        if let Some((doc, _)) = email_lookup {
            let endpoint = provider
                .email_endpoint
                .as_ref()
                .expect("email endpoint configured");
            if endpoint.primary_array {
                let items = doc.as_array().cloned().unwrap_or_default();
                let chosen = items
                    .iter()
                    .find(|e| lookup(e, "primary").is_some_and(truthy))
                    .or_else(|| items.first());
                if let Some(chosen) = chosen {
                    email = string_at(chosen, &endpoint.path);
                    if let Some(v) = &endpoint.verified_path {
                        email_verified = lookup(chosen, v).is_some_and(truthy);
                    }
                }
            } else {
                email = string_at(doc, &endpoint.path);
                if email.is_some() {
                    email_verified = true;
                }
            }
        }
    }
    // No address from the provider: a placeholder keyed to the account, so
    // the unique index holds and the address is visibly not real.
    let email = email.unwrap_or_else(|| synthetic_email(&provider.id, &id));

    let image = match provider.image_fixup {
        ImageFixup::None => first_string(source, &provider.image_paths),
        ImageFixup::StripQuery => first_string(source, &provider.image_paths)
            .map(|raw| raw.split('?').next().unwrap_or(&raw).to_string()),
        ImageFixup::DiscordAvatar => Some(discord_avatar(source, &provider.image_paths, &id)),
    };

    OAuthProfile {
        id,
        name,
        email: Some(email),
        image,
        email_verified,
        raw: source.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosql::Object;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn every_better_auth_provider_is_present() {
        // The provider ids Better Auth ships, one for one. A provider that
        // is missing here is a provider an app cannot migrate.
        let better_auth = [
            "apple",
            "atlassian",
            "cloudflare",
            "cognito",
            "discord",
            "dropbox",
            "facebook",
            "figma",
            "github",
            "gitlab",
            "google",
            "huggingface",
            "kakao",
            "kick",
            "line",
            "linear",
            "linkedin",
            "microsoft",
            "naver",
            "notion",
            "paybin",
            "paypal",
            "polar",
            "railway",
            "reddit",
            "roblox",
            "salesforce",
            "slack",
            "spotify",
            "tiktok",
            "twitch",
            "twitter",
            "vercel",
            "vk",
            "wechat",
            "zoom",
        ];
        for id in better_auth {
            assert!(preset(id).is_some(), "no preset for {id}");
        }
        assert_eq!(PRESETS.len(), better_auth.len());
    }

    #[test]
    fn wechat_uses_its_own_names() {
        let empty = BTreeMap::new();
        let wechat =
            ProviderConfig::from_preset(preset("wechat").unwrap(), "wx-id", "wx-secret", &empty)
                .unwrap();
        // The authorization URL names the client `appid` and ends in the
        // fragment WeChat needs.
        let url = wechat.authorization_url(
            "https://app.test/api/auth/callback/wechat",
            "state123",
            None,
            &[],
            None,
            None,
        );
        assert!(url.contains("appid=wx-id"), "{url}");
        assert!(url.contains("#wechat_redirect"), "{url}");
        assert!(!url.contains("client_id="), "{url}");
        // The token request is a GET with `secret`, and neither
        // `redirect_uri` nor `code_verifier`.
        let (_, body, _) = wechat.token_request("the-code", "https://app.test/cb", Some("v"));
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("secret=wx-secret"), "{body}");
        assert!(body.contains("appid=wx-id"), "{body}");
        assert!(!body.contains("redirect_uri"), "{body}");
        assert!(!body.contains("code_verifier"), "{body}");
        // The userinfo request carries the `openid` from the token response.
        let mut token = Object::new();
        token.set("openid", Value::str("o-123"));
        let request = wechat
            .userinfo_request("tok", &Value::Object(token))
            .unwrap();
        assert!(request.url.contains("openid=o-123"), "{}", request.url);
        assert!(request.url.contains("access_token=tok"), "{}", request.url);
    }

    #[test]
    fn cloudflare_reads_the_result_envelope() {
        let empty = BTreeMap::new();
        let cloudflare =
            ProviderConfig::from_preset(preset("cloudflare").unwrap(), "cf", "secret", &empty)
                .unwrap();
        let mut result = Object::new();
        result.set("id", Value::str("cf-1"));
        result.set("email", Value::str("ada@example.com"));
        result.set("first_name", Value::str("Ada"));
        result.set("last_name", Value::str("Lovelace"));
        let mut envelope = Object::new();
        envelope.set("result", Value::Object(result));
        let profile = extract_profile(&cloudflare, &Value::Null, &Value::Object(envelope), None);
        assert_eq!(profile.id, "cf-1");
        assert_eq!(profile.email.as_deref(), Some("ada@example.com"));
        assert_eq!(profile.name, "Ada Lovelace");
    }

    #[test]
    fn vercel_asks_for_no_scopes() {
        let empty = BTreeMap::new();
        let vercel =
            ProviderConfig::from_preset(preset("vercel").unwrap(), "v", "s", &empty).unwrap();
        assert!(vercel.scopes.is_empty());
        let url = vercel.authorization_url(
            "https://app.test/api/auth/callback/vercel",
            "state123",
            None,
            &[],
            None,
            None,
        );
        assert!(!url.contains("scope="), "{url}");
    }

    #[test]
    fn every_preset_is_well_formed_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for p in PRESETS {
            assert!(seen.insert(p.id), "duplicate preset {}", p.id);
            assert!(
                p.authorization_url.starts_with("https://") || p.authorization_url.starts_with('{'),
                "{}",
                p.id
            );
            assert!(
                p.token_url.starts_with("https://") || p.token_url.starts_with('{'),
                "{}",
                p.id
            );
            assert!(!p.id_paths.is_empty(), "{} needs an id path", p.id);
            assert!(p.client_id_env.ends_with("_CLIENT_ID"), "{}", p.id);
            assert!(p.client_secret_env.ends_with("_CLIENT_SECRET"), "{}", p.id);
            assert!(
                matches!(
                    p.profile,
                    ProfileSource::IdToken | ProfileSource::TokenResponse
                ) || p.userinfo_url.is_some(),
                "{} fetches a profile but has no userinfo url",
                p.id
            );
        }
        let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        for expected in [
            "google",
            "github",
            "apple",
            "atlassian",
            "cognito",
            "discord",
            "facebook",
            "figma",
            "line",
            "huggingface",
            "kakao",
            "kick",
            "microsoft",
            "paypal",
            "salesforce",
            "slack",
            "notion",
            "naver",
            "tiktok",
            "twitch",
            "twitter",
            "dropbox",
            "linear",
            "linkedin",
            "gitlab",
            "reddit",
            "roblox",
            "spotify",
            "vk",
            "zoom",
            "polar",
            "paybin",
        ] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn registry_loads_from_env_and_skips_unconfigured() {
        let (registry, problems) = OAuthRegistry::from_env_with(env(&[
            ("GOOGLE_CLIENT_ID", "g-id"),
            ("GOOGLE_CLIENT_SECRET", "g-secret"),
            ("GITHUB_CLIENT_ID", "gh-id"),
            ("COGNITO_CLIENT_ID", "c-id"),
            ("COGNITO_CLIENT_SECRET", "c-secret"),
            ("COGNITO_DOMAIN", "auth.eu-west-1.test"),
            ("MICROSOFT_CLIENT_ID", "m-id"),
            ("MICROSOFT_CLIENT_SECRET", "m-secret"),
            ("TENANT_X", "x"),
        ]));
        assert_eq!(registry.ids(), vec!["cognito", "google", "microsoft"]);
        // Half-configured GitHub is a problem worth reporting.
        assert!(problems.iter().any(
            |p| matches!(p, ConfigError::MissingCredential { provider, variable }
                if provider == "github" && variable == "GITHUB_CLIENT_SECRET")
        ));
        let google = registry.get("google").unwrap();
        assert_eq!(
            google.authorization_url,
            "https://accounts.google.com/o/oauth2/v2/auth"
        );
        assert_eq!(google.scopes, vec!["email", "profile", "openid"]);
        assert_eq!(google.profile, ProfileSource::IdToken);
        let ms = registry.get("microsoft").unwrap();
        assert!(
            ms.token_url
                .starts_with("https://login.microsoftonline.com/common/"),
            "{}",
            ms.token_url
        );
        assert_eq!(
            registry.get("cognito").unwrap().token_url,
            "https://auth.eu-west-1.test/oauth2/token"
        );
        // Cognito without a domain is not usable, and says so.
        let (bare, problems) = OAuthRegistry::from_env_with(env(&[
            ("COGNITO_CLIENT_ID", "c"),
            ("COGNITO_CLIENT_SECRET", "s"),
        ]));
        assert!(bare.is_empty());
        assert_eq!(
            problems,
            vec![ConfigError::MissingVariable {
                provider: "cognito".into(),
                placeholder: "domain".into()
            }]
        );
        assert_eq!(problems[0].to_string(), "cognito: no value for {domain}");
    }

    #[test]
    fn templated_providers_resolve_their_hosts() {
        let vars: BTreeMap<String, String> =
            [("domain".to_string(), "auth.example.app".to_string())]
                .into_iter()
                .collect();
        let cognito =
            ProviderConfig::from_preset(preset("cognito").unwrap(), "id", "secret", &vars).unwrap();
        assert_eq!(cognito.token_url, "https://auth.example.app/oauth2/token");
        assert_eq!(
            cognito.userinfo_url.as_deref(),
            Some("https://auth.example.app/oauth2/userinfo")
        );
        assert!(ProviderConfig::from_preset(
            preset("cognito").unwrap(),
            "id",
            "secret",
            &BTreeMap::new()
        )
        .is_err());
        assert_eq!(first_placeholder("{issuer}/oauth/token"), "issuer");
        let gitlab = ProviderConfig::from_preset(
            preset("gitlab").unwrap(),
            "id",
            "secret",
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            gitlab.userinfo_url.as_deref(),
            Some("https://gitlab.com/api/v4/user")
        );
        let (sandbox_registry, _) = OAuthRegistry::from_env_with(env(&[
            ("PAYPAL_CLIENT_ID", "p"),
            ("PAYPAL_CLIENT_SECRET", "s"),
            ("PAYPAL_MODE", "sandbox"),
        ]));
        let paypal = sandbox_registry.get("paypal").unwrap();
        assert_eq!(
            paypal.token_url,
            "https://api-m.sandbox.paypal.com/v1/oauth2/token"
        );
        assert_eq!(
            paypal.authorization_url,
            "https://www.sandbox.paypal.com/signin/authorize"
        );
        let (live_registry, _) = OAuthRegistry::from_env_with(env(&[
            ("PAYPAL_CLIENT_ID", "p"),
            ("PAYPAL_CLIENT_SECRET", "s"),
        ]));
        assert_eq!(
            live_registry.get("paypal").unwrap().token_url,
            "https://api-m.paypal.com/v1/oauth2/token"
        );
    }

    #[test]
    fn authorization_url_carries_pkce_state_and_provider_quirks() {
        let empty = BTreeMap::new();
        let google =
            ProviderConfig::from_preset(preset("google").unwrap(), "g-id", "s", &empty).unwrap();
        let url = google.authorization_url(
            "https://app.test/api/auth/callback/google",
            "state123",
            Some("challenge"),
            &[],
            Some("ada@example.com"),
            None,
        );
        assert!(
            url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
            "{url}"
        );
        for expected in [
            "response_type=code",
            "client_id=g-id",
            "redirect_uri=https%3A%2F%2Fapp.test%2Fapi%2Fauth%2Fcallback%2Fgoogle",
            "state=state123",
            "scope=email%20profile%20openid",
            "code_challenge=challenge",
            "code_challenge_method=S256",
            "login_hint=ada%40example.com",
        ] {
            assert!(url.contains(expected), "{url} lacks {expected}");
        }
        // Explicit scopes replace the defaults; the challenge is optional.
        let url = google.authorization_url(
            "https://app.test/cb",
            "st",
            None,
            &["email".into()],
            None,
            None,
        );
        assert!(url.ends_with("scope=email"), "{url}");
        assert!(!url.contains("code_challenge"));
        assert!(!url.contains("login_hint"));

        let apple =
            ProviderConfig::from_preset(preset("apple").unwrap(), "a", "s", &empty).unwrap();
        let url = apple.authorization_url("https://app.test/cb", "st", Some("c"), &[], None, None);
        assert!(url.contains("response_mode=form_post"));
        assert!(url.contains("response_type=code%20id_token"));
        assert!(url.contains("scope=email%20name"));

        let tiktok =
            ProviderConfig::from_preset(preset("tiktok").unwrap(), "key", "s", &empty).unwrap();
        let url = tiktok.authorization_url("https://app.test/cb", "st", Some("c"), &[], None, None);
        assert!(url.contains("client_key=key"), "{url}");
        assert!(!url.contains("client_id="));

        let notion =
            ProviderConfig::from_preset(preset("notion").unwrap(), "n", "s", &empty).unwrap();
        let url = notion.authorization_url("https://app.test/cb", "st", Some("c"), &[], None, None);
        assert!(url.contains("owner=user"));
        assert!(!url.contains("scope="));

        let discord =
            ProviderConfig::from_preset(preset("discord").unwrap(), "d", "s", &empty).unwrap();
        assert!(discord
            .authorization_url("https://app.test/cb", "st", None, &[], None, None)
            .contains("prompt=none"));
        let roblox =
            ProviderConfig::from_preset(preset("roblox").unwrap(), "r", "s", &empty).unwrap();
        assert!(roblox
            .authorization_url("https://app.test/cb", "st", None, &[], None, None)
            .contains("prompt=select_account%20consent"));
    }

    #[test]
    fn token_request_auth_methods() {
        let empty = BTreeMap::new();
        let github =
            ProviderConfig::from_preset(preset("github").unwrap(), "gh", "secret", &empty).unwrap();
        let (_, body, headers) =
            github.token_request("the-code", "https://app.test/cb", Some("verifier"));
        let body = String::from_utf8(body).unwrap();
        assert_eq!(body, "grant_type=authorization_code&code=the-code&redirect_uri=https%3A%2F%2Fapp.test%2Fcb&code_verifier=verifier&client_id=gh&client_secret=secret");
        assert!(!headers.iter().any(|(k, _)| k == "authorization"));

        let reddit =
            ProviderConfig::from_preset(preset("reddit").unwrap(), "id", "pw", &empty).unwrap();
        let (_, _, headers) = reddit.token_request("c", "https://app.test/cb", None);
        let auth = headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(auth, "Basic aWQ6cHc=");

        let notion =
            ProviderConfig::from_preset(preset("notion").unwrap(), "n", "s", &empty).unwrap();
        let (_, body, headers) = notion.token_request("c", "https://app.test/cb", Some("v"));
        assert!(headers.contains(&("content-type".into(), "application/json".into())));
        let json = mosql::parse_json(std::str::from_utf8(&body).unwrap()).unwrap();
        assert_eq!(
            json.get("grant_type"),
            Some(&Value::str("authorization_code"))
        );
        assert_eq!(json.get("code_verifier"), Some(&Value::str("v")));
        assert_eq!(json.get("client_secret"), None);

        let tiktok =
            ProviderConfig::from_preset(preset("tiktok").unwrap(), "key", "s", &empty).unwrap();
        let (_, body, _) = tiktok.token_request("c", "https://app.test/cb", None);
        // TikTok's client parameter is `client_key`, and it is sent once.
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("client_key=key"), "{body}");
        assert!(!body.contains("client_id"), "{body}");

        let (refresh, _) = github.refresh_request("rt");
        assert_eq!(String::from_utf8(refresh).unwrap(), "grant_type=refresh_token&refresh_token=rt&scope=read%3Auser%20user%3Aemail&client_id=gh&client_secret=secret");
    }

    #[test]
    fn userinfo_requests_carry_the_right_credentials() {
        let empty = BTreeMap::new();
        let github =
            ProviderConfig::from_preset(preset("github").unwrap(), "gh", "s", &empty).unwrap();
        let req = github.userinfo_request("tok", &Value::Null).unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.url, "https://api.github.com/user");
        assert!(req
            .headers
            .contains(&("user-agent".into(), "better-auth".into())));
        assert!(req
            .headers
            .contains(&("authorization".into(), "Bearer tok".into())));

        let twitch =
            ProviderConfig::from_preset(preset("twitch").unwrap(), "t", "s", &empty).unwrap();
        let req = twitch.userinfo_request("tok", &Value::Null).unwrap();
        assert!(
            req.headers.contains(&("Client-Id".into(), "t".into())),
            "{:?}",
            req.headers
        );

        let linear =
            ProviderConfig::from_preset(preset("linear").unwrap(), "l", "s", &empty).unwrap();
        let req = linear.userinfo_request("tok", &Value::Null).unwrap();
        assert_eq!(req.method, Method::Post);
        assert!(String::from_utf8(req.body).unwrap().contains("viewer"));

        let vk = ProviderConfig::from_preset(preset("vk").unwrap(), "v", "s", &empty).unwrap();
        let req = vk.userinfo_request("tok", &Value::Null).unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(
            String::from_utf8(req.body).unwrap(),
            "access_token=tok&client_id=v"
        );
        assert!(!req.headers.iter().any(|(k, _)| k == "authorization"));

        let paypal =
            ProviderConfig::from_preset(preset("paypal").unwrap(), "p", "s", &empty).unwrap();
        let req = paypal.userinfo_request("tok", &Value::Null).unwrap();
        assert!(
            req.url.ends_with("userinfo?schema=paypalv1.1"),
            "{}",
            req.url
        );

        let google =
            ProviderConfig::from_preset(preset("google").unwrap(), "g", "s", &empty).unwrap();
        assert!(google.userinfo_request("tok", &Value::Null).is_none());
    }

    #[test]
    fn profiles_are_read_by_dot_path() {
        let empty = BTreeMap::new();
        let json = |s: &str| mosql::parse_json(s).unwrap();

        // Arrays by index, nested objects, a synthetic address when none.
        let kick = ProviderConfig::from_preset(preset("kick").unwrap(), "k", "s", &empty).unwrap();
        let p = extract_profile(
            &kick,
            &Value::Null,
            &json(
                r#"{"data":[{"id":"77","name":"Ada","email":"","profile_picture":"https://p/x.png"}]}"#,
            ),
            None,
        );
        assert_eq!(p.id, "77");
        assert_eq!(p.name, "Ada");
        assert_eq!(p.email.as_deref(), Some("kick_77@oauth.local"));
        assert_eq!(p.image.as_deref(), Some("https://p/x.png"));
        assert!(!p.email_verified);

        // Verified flags in their various spellings.
        let apple =
            ProviderConfig::from_preset(preset("apple").unwrap(), "a", "s", &empty).unwrap();
        let p = extract_profile(
            &apple,
            &Value::Null,
            &json(r#"{"sub":"001","email":"a@x.co","email_verified":"true"}"#),
            None,
        );
        assert!(p.email_verified);
        assert_eq!(p.name, "");

        // GitHub: the profile has no email, so the emails document is used.
        let github =
            ProviderConfig::from_preset(preset("github").unwrap(), "g", "s", &empty).unwrap();
        let emails = json(
            r#"[{"email":"secondary@x.co","primary":false,"verified":true},{"email":"primary@x.co","primary":true,"verified":false}]"#,
        );
        let p = extract_profile(
            &github,
            &Value::Null,
            &json(r#"{"id":"1","login":"ada","name":"","avatar_url":"https://a"}"#),
            Some((&emails, true)),
        );
        assert_eq!(p.email.as_deref(), Some("primary@x.co"));
        assert_eq!(p.name, "ada");
        assert!(!p.email_verified);
        let p = extract_profile(
            &github,
            &Value::Null,
            &json(r#"{"id":"1","login":"ada","email":"direct@x.co"}"#),
            Some((&emails, true)),
        );
        assert_eq!(p.email.as_deref(), Some("direct@x.co"));

        // Twitter's confirmed address is a separate document.
        let twitter =
            ProviderConfig::from_preset(preset("twitter").unwrap(), "t", "s", &empty).unwrap();
        let confirmed = json(r#"{"data":{"confirmed_email":"c@x.co"}}"#);
        let p = extract_profile(
            &twitter,
            &Value::Null,
            &json(r#"{"data":{"id":"9","name":"Ada","profile_image_url":"https://p"}}"#),
            Some((&confirmed, true)),
        );
        assert_eq!(p.email.as_deref(), Some("c@x.co"));
        assert!(p.email_verified);
        let p = extract_profile(
            &twitter,
            &Value::Null,
            &json(r#"{"data":{"id":"9","name":"Ada"}}"#),
            Some((&json(r#"{"data":{}}"#), true)),
        );
        assert_eq!(p.email.as_deref(), Some("twitter_9@oauth.local"));

        // Notion reads its profile out of the token response.
        let notion =
            ProviderConfig::from_preset(preset("notion").unwrap(), "n", "s", &empty).unwrap();
        let token = json(
            r#"{"access_token":"secret_x","owner":{"type":"user","user":{"id":"u1","name":"Ada","avatar_url":"https://a","person":{"email":"ada@x.co"}}}}"#,
        );
        let p = extract_profile(&notion, &token, &Value::Null, None);
        assert_eq!(p.id, "u1");
        assert_eq!(p.email.as_deref(), Some("ada@x.co"));
        assert_eq!(p.raw, token);

        // VK joins first and last names, takes the email from the token.
        let vk = ProviderConfig::from_preset(preset("vk").unwrap(), "v", "s", &empty).unwrap();
        let p = extract_profile(
            &vk,
            &json(r#"{"email":"vk@x.co"}"#),
            &json(
                r#"{"user":{"id":5,"first_name":"Ada","last_name":"Lovelace","avatar":"https://a"}}"#,
            ),
            None,
        );
        assert_eq!(p.name, "Ada Lovelace");
        assert_eq!(p.id, "5");
        assert_eq!(p.email.as_deref(), Some("vk@x.co"));

        // Discord composes the avatar URL, and falls back to the default.
        let discord =
            ProviderConfig::from_preset(preset("discord").unwrap(), "d", "s", &empty).unwrap();
        let p = extract_profile(
            &discord,
            &Value::Null,
            &json(
                r#"{"id":"42","global_name":"Ada","avatar":"a_abc","email":"d@x.co","verified":true}"#,
            ),
            None,
        );
        assert_eq!(
            p.image.as_deref(),
            Some("https://cdn.discordapp.com/avatars/42/a_abc.gif")
        );
        assert!(p.email_verified);
        let p = extract_profile(
            &discord,
            &Value::Null,
            &json(r#"{"id":"42","username":"ada","avatar":null}"#),
            None,
        );
        assert_eq!(p.name, "ada");
        assert!(p
            .image
            .unwrap()
            .starts_with("https://cdn.discordapp.com/embed/avatars/"));

        // Reddit strips the size query from its icon.
        let reddit =
            ProviderConfig::from_preset(preset("reddit").unwrap(), "r", "s", &empty).unwrap();
        let p = extract_profile(
            &reddit,
            &Value::Null,
            &json(r#"{"id":"t2_x","name":"ada","icon_img":"https://a/b.png?width=64"}"#),
            None,
        );
        assert_eq!(p.image.as_deref(), Some("https://a/b.png"));

        // Spotify's first image is an array element.
        let spotify =
            ProviderConfig::from_preset(preset("spotify").unwrap(), "s", "s", &empty).unwrap();
        let p = extract_profile(
            &spotify,
            &Value::Null,
            &json(
                r#"{"id":"s1","display_name":"Ada","email":"s@x.co","images":[{"url":"https://i"},{"url":"https://j"}]}"#,
            ),
            None,
        );
        assert_eq!(p.image.as_deref(), Some("https://i"));
    }

    #[test]
    fn a_provider_can_be_described_by_the_environment() {
        let (providers, problems) = OAuthRegistry::from_env_generic(
            |name| match name {
                "OAUTH_ACME_CLIENT_ID" => Some("cid".into()),
                "OAUTH_ACME_CLIENT_SECRET" => Some("sec".into()),
                "OAUTH_ACME_DISCOVERY_URL" => {
                    Some("https://acme.test/.well-known/openid-configuration".into())
                }
                _ => None,
            },
            |_| vec!["OAUTH_ACME_CLIENT_ID".into()],
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(providers.len(), 1);
        let acme = &providers[0];
        assert_eq!(acme.id, "acme");
        assert_eq!(acme.client_id, "cid");
        assert_eq!(
            acme.discovery_url.as_deref(),
            Some("https://acme.test/.well-known/openid-configuration")
        );
        // The usual scopes, unless the environment says otherwise.
        assert_eq!(acme.scopes, vec!["openid", "profile", "email"]);
    }

    #[test]
    fn a_half_described_provider_is_reported() {
        let (providers, problems) = OAuthRegistry::from_env_generic(
            |name| match name {
                "OAUTH_ACME_CLIENT_ID" => Some("cid".into()),
                _ => None,
            },
            |_| vec!["OAUTH_ACME_CLIENT_ID".into()],
        );
        assert!(providers.is_empty());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].to_string().contains("CLIENT_SECRET"));
    }

    #[test]
    fn a_provider_with_only_a_secret_names_the_id_that_is_missing() {
        let (providers, problems) = OAuthRegistry::from_env_generic(
            |name| match name {
                "OAUTH_ACME_CLIENT_SECRET" => Some("sec".into()),
                _ => None,
            },
            |_| vec!["OAUTH_ACME_CLIENT_SECRET".into()],
        );
        assert!(providers.is_empty());
        assert_eq!(problems.len(), 1);
        let problem = problems[0].to_string();
        assert!(problem.contains("OAUTH_ACME_CLIENT_ID"), "{problem}");
        assert!(!problem.contains("CLIENT_SECRET"), "{problem}");
    }

    #[test]
    fn a_provider_without_any_endpoint_is_reported() {
        let (providers, problems) = OAuthRegistry::from_env_generic(
            |name| match name {
                "OAUTH_ACME_CLIENT_ID" => Some("cid".into()),
                "OAUTH_ACME_CLIENT_SECRET" => Some("sec".into()),
                _ => None,
            },
            |_| vec!["OAUTH_ACME_CLIENT_ID".into()],
        );
        assert!(providers.is_empty());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].to_string().contains("DISCOVERY_URL"));
    }

    #[test]
    fn a_custom_provider_keeps_the_usual_paths_when_it_names_none() {
        let config = ProviderConfig::from_custom(&CustomProvider {
            id: "acme".into(),
            client_id: "cid".into(),
            client_secret: "sec".into(),
            authorization_url: "https://sso.acme.test/authorize".into(),
            token_url: "https://sso.acme.test/token".into(),
            ..CustomProvider::default()
        });
        let profile = mosql::parse_json(
            r#"{"sub":"1","name":"Ada","email":"ada@acme.test","email_verified":true,"picture":"https://i"}"#,
        )
        .unwrap();
        let p = extract_profile(&config, &Value::Null, &profile, None);
        assert_eq!(p.id, "1");
        assert_eq!(p.name, "Ada");
        assert_eq!(p.email.as_deref(), Some("ada@acme.test"));
        assert!(p.email_verified);
        assert_eq!(p.image.as_deref(), Some("https://i"));
    }

    #[test]
    fn custom_providers_join_the_registry() {
        let mut registry = OAuthRegistry::new();
        registry.insert(ProviderConfig::from_custom(&CustomProvider {
            id: "acme".into(),
            display_name: "Acme".into(),
            client_id: "cid".into(),
            client_secret: "sec".into(),
            authorization_url: "https://sso.acme.test/authorize".into(),
            token_url: "https://sso.acme.test/token".into(),
            userinfo_url: Some("https://sso.acme.test/me".into()),
            scopes: vec!["openid".into(), "email".into()],
            pkce: true,
            token_auth: Some(TokenAuth::Basic),
            id_path: Some("sub".into()),
            name_path: Some("displayName".into()),
            email_path: Some("mail".into()),
            image_path: None,
            verified_path: Some("emailVerified".into()),
            discovery_url: None,
            issuer: None,
            require_id_token_verification: false,
            disable_id_token_nonce_binding: false,
            token_method: None,
            token_body: None,
            client_id_param: None,
            client_secret_param: None,
            token_extra_fields: Vec::new(),
            userinfo_method: None,
            userinfo_auth: None,
            scope_separator: None,
            response_type: None,
            prompt: None,
        }));
        assert_eq!(registry.ids(), vec!["acme"]);
        let acme = registry.get("acme").unwrap();
        let p = extract_profile(
            acme,
            &Value::Null,
            &mosql::parse_json(
                r#"{"sub":"1","displayName":"Ada","mail":"ada@acme.test","emailVerified":true}"#,
            )
            .unwrap(),
            None,
        );
        assert_eq!(p.id, "1");
        assert_eq!(p.name, "Ada");
        assert_eq!(p.email.as_deref(), Some("ada@acme.test"));
        assert!(p.email_verified);
    }

    #[test]
    fn base64_matches_rfc_4648() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foob"), "Zm9vYg==");
        assert_eq!(b64(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64("id:pw".as_bytes()), "aWQ6cHc=");
    }
}
