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
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"who2ban\"")],
        )
            .into_response();
    }

    next.run(request).await
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
