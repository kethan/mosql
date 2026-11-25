//! The console: the platform's own web interface.
//!
//! The three files are compiled into the binary, so there is nothing to
//! install, nothing to build, and nothing to deploy next to the
//! executable. A console that needed a folder beside the binary would be
//! one more thing to forget, and the whole point of the platform is that
//! one file is the whole of it.
//!
//! The console is served from the same origin as the API, so it needs no
//! CORS of its own — a browser asking the API for the API's own origin is
//! same-origin, and same-origin requests are not restricted. CORS is for
//! the *other* pages: a front end served from somewhere else, or a file
//! opened from disk.

use crate::http::{Request, Response};

/// The console's page.
const INDEX: &str = include_str!("../ui/index.html");
/// The console's stylesheet.
const CSS: &str = include_str!("../ui/console.css");
/// The console's script.
const JS: &str = include_str!("../ui/console.js");

/// The console's file for a path.
///
/// The answer is the content type, the body and how long it may be
/// cached. `no-store` for the page, because it is the shell and must not
/// be held on to across a deployment; `no-cache` for the assets, which
/// are revalidated every time and so cost nothing when they have not
/// changed.
pub fn asset(path: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match path {
        "/" | "/index.html" => Some(("text/html; charset=utf-8", INDEX, "no-store")),
        "/console.css" => Some(("text/css; charset=utf-8", CSS, "no-cache")),
        "/console.js" => Some(("text/javascript; charset=utf-8", JS, "no-cache")),
        _ => None,
    }
}

/// The console's answer to a request, if the request is for the console.
///
/// The content policy is the console's own: it may load its own script
/// and its own stylesheet, and nothing else. An API answer gets the
/// stricter policy, which forbids loading anything at all.
pub fn respond(req: &Request) -> Option<Response> {
    let (content_type, body, cache) = asset(&req.path)?;
    let response = Response {
        status: 200,
        headers: vec![
            ("content-type".to_string(), content_type.to_string()),
            ("cache-control".to_string(), cache.to_string()),
        ],
        body: body.as_bytes().to_vec(),
    };
    Some(crate::middleware::secure(response, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_the_console() {
        assert!(asset("/").is_some());
        assert!(asset("/index.html").is_some());
    }

    #[test]
    fn the_assets_are_the_console() {
        let (kind, body, _) = asset("/console.js").expect("the script");
        assert!(kind.starts_with("text/javascript"));
        assert!(body.contains("/api/apps"));
        let (kind, body, _) = asset("/console.css").expect("the stylesheet");
        assert!(kind.starts_with("text/css"));
        assert!(body.contains("@media"));
    }

    #[test]
    fn anything_else_is_not_the_console() {
        assert!(asset("/api/apps").is_none());
        assert!(asset("/nope").is_none());
    }

    #[test]
    fn the_console_is_served_with_its_own_policy() {
        let req = Request::new("GET", "/");
        let response = respond(&req).expect("the console");
        let csp = response
            .headers
            .iter()
            .find(|(k, _)| k == "content-security-policy")
            .map(|(_, v)| v.clone())
            .expect("a content policy");
        assert!(
            csp.contains("script-src 'self'"),
            "the policy is the console's"
        );
        assert!(csp.contains("frame-ancestors 'none'"));
    }
}
