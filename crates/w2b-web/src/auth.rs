use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The header a client must send, or `None` when no login is configured.
pub fn wanted() -> Option<String> {
    let user = std::env::var("BASIC_USER").ok().filter(|u| !u.is_empty())?;
    let pass = std::env::var("BASIC_PASS").unwrap_or_default();
    let login = format!("{user}:{pass}");
    Some(format!(
        "Basic {}",
        w2b_core::base64::encode(login.as_bytes())
    ))
}

pub async fn guard(State(wanted): State<String>, request: Request, next: Next) -> Response {
    let sent = request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|value| value.as_bytes())
        .unwrap_or_default();

    if !same(sent, wanted.as_bytes()) {
        // Said out loud, because a login nobody watches is a login nobody defends. One
        // shared password on a public hostname is guessable given enough tries, and
        // until now every one of those tries was silent - there was nothing in the log
        // to tell a wrong password from a thousand of them.
        //
        // Never what was sent: a mistyped password is still a password, and a log is a
        // file that gets copied around. Only that it happened, and who from.
        tracing::warn!(
            from = %asker(&request),
            path = %request.uri().path(),
            offered = !sent.is_empty(),
            "refused"
        );
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"who2ban\"")],
        )
            .into_response();
    }

    next.run(request).await
}

/// Where a request came from, as well as this can be known. Nothing listens on a public
/// port: the tunnel dials out and every request arrives from it, so the socket's own
/// address is the tunnel every time and the caller is only ever named in a header.
/// Headers are the client's to write, so this is for reading a log, never for deciding
/// anything.
fn asker(request: &Request) -> String {
    ["cf-connecting-ip", "x-forwarded-for"]
        .iter()
        .find_map(|name| {
            request
                .headers()
                .get(*name)?
                .to_str()
                .ok()
                .map(|v| v.chars().take(64).collect())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Reads every byte whatever the first one says: the time it takes leaks the length only.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_becomes_a_basic_auth_header() {
        unsafe {
            std::env::set_var("BASIC_USER", "me");
            std::env::set_var("BASIC_PASS", "secret");
        }
        assert_eq!(wanted().as_deref(), Some("Basic bWU6c2VjcmV0"));

        unsafe { std::env::set_var("BASIC_USER", "") };
        assert_eq!(wanted(), None);
    }

    #[test]
    fn same_holds_on_a_length_it_never_read() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"", b"abc"));
        assert!(!same(b"abcd", b"abc"));
    }
}
