//! Outgoing email port (T-22): magic links today, other transactional mail later (it moves to the
//! notifications module when that exists). Adapters:
//!
//! - [`SmtpMailer`]: plain SMTP without TLS or auth, for Mailpit in local development only.
//! - [`PostmarkMailer`]: Postmark's HTTPS API (`POST /email`), behind config. Written against
//!   Postmark's documented API; not yet run against the real service (no account yet).
//! - [`RecordingMailer`] (test support): keeps messages in memory.
//!
//! Messages are plain text. Addresses are validated so no header can be injected.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use platform::config::Secret;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::oidc::ProviderHttp;

/// One plain-text email.
#[derive(Clone, PartialEq, Eq)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
}

impl std::fmt::Debug for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The body carries one-time links: never print it.
        f.debug_struct("Email")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// Worth retrying (network, 5xx, timeout).
    #[error("temporary mail failure: {0}")]
    Temporary(String),
    /// Retrying cannot help (invalid address, rejected by the provider, not configured).
    #[error("permanent mail failure: {0}")]
    Permanent(String),
}

pub type MailFuture<'a> = Pin<Box<dyn Future<Output = Result<(), MailError>> + Send + 'a>>;

pub trait Mailer: Send + Sync {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a>;
}

/// Used when no transport is configured: every send fails permanently (and is logged).
pub struct UnconfiguredMailer;

impl Mailer for UnconfiguredMailer {
    fn send<'a>(&'a self, _email: &'a Email) -> MailFuture<'a> {
        Box::pin(async {
            Err(MailError::Permanent(
                "mail transport not configured".to_owned(),
            ))
        })
    }
}

/// An address safe to put in an SMTP envelope and headers.
pub fn valid_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    (3..=320).contains(&address.len())
        && !local.is_empty()
        && local.len() <= 64
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && address.chars().all(|c| {
            c.is_ascii_graphic() && !matches!(c, '<' | '>' | '(' | ')' | ',' | ';' | '"' | '\\')
        })
        && !domain.contains('@')
}

const SMTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Plain SMTP to a local catcher (Mailpit). No TLS, no auth: never point it at a real relay.
pub struct SmtpMailer {
    host: String,
    port: u16,
    from: String,
}

impl SmtpMailer {
    pub fn new(host: String, port: u16, from: String) -> Self {
        Self { host, port, from }
    }

    async fn deliver(&self, email: &Email) -> Result<(), MailError> {
        let stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|err| MailError::Temporary(format!("connect: {err}")))?;
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        expect_reply(&mut read, 220).await?;
        command(&mut write, &mut read, "EHLO card-sharing.local", 250).await?;
        command(
            &mut write,
            &mut read,
            &format!("MAIL FROM:<{}>", self.from),
            250,
        )
        .await?;
        command(
            &mut write,
            &mut read,
            &format!("RCPT TO:<{}>", email.to),
            250,
        )
        .await?;
        command(&mut write, &mut read, "DATA", 354).await?;
        let message = format!(
            "From: <{}>\r\nTo: <{}>\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{}\r\n.\r\n",
            self.from,
            email.to,
            email.subject,
            dot_stuff(&email.text),
        );
        write
            .write_all(message.as_bytes())
            .await
            .map_err(|err| MailError::Temporary(err.to_string()))?;
        expect_reply(&mut read, 250).await?;
        // Best effort: the message is already accepted.
        let _ = write.write_all(b"QUIT\r\n").await;
        Ok(())
    }
}

impl Mailer for SmtpMailer {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a> {
        Box::pin(async move {
            check_message(&self.from, email)?;
            tokio::time::timeout(SMTP_TIMEOUT, self.deliver(email))
                .await
                .map_err(|_| MailError::Temporary("SMTP timed out".to_owned()))?
        })
    }
}

async fn command<W, R>(write: &mut W, read: &mut R, line: &str, code: u16) -> Result<(), MailError>
where
    W: AsyncWriteExt + Unpin,
    R: AsyncBufReadExt + Unpin,
{
    write
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .map_err(|err| MailError::Temporary(err.to_string()))?;
    expect_reply(read, code).await
}

/// Reads a (possibly multi-line) reply and checks its code.
async fn expect_reply<R: AsyncBufReadExt + Unpin>(
    read: &mut R,
    code: u16,
) -> Result<(), MailError> {
    loop {
        let mut line = String::new();
        let n = read
            .read_line(&mut line)
            .await
            .map_err(|err| MailError::Temporary(err.to_string()))?;
        if n == 0 || line.len() < 4 {
            return Err(MailError::Temporary("SMTP connection closed".to_owned()));
        }
        let got: u16 = line[..3].parse().unwrap_or(0);
        if line.as_bytes()[3] == b'-' {
            continue;
        }
        if got == code || (code == 250 && got == 251) {
            return Ok(());
        }
        let reply = line.trim_end().to_owned();
        return Err(if (500..600).contains(&got) {
            MailError::Permanent(reply)
        } else {
            MailError::Temporary(reply)
        });
    }
}

/// Lines starting with `.` get an extra `.` (RFC 5321 §4.5.2); line endings become CRLF.
fn dot_stuff(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with('.') {
                format!(".{line}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn check_message(from: &str, email: &Email) -> Result<(), MailError> {
    if !valid_address(from) || !valid_address(&email.to) {
        return Err(MailError::Permanent("invalid address".to_owned()));
    }
    if email.subject.contains(['\r', '\n']) || !email.subject.is_ascii() {
        return Err(MailError::Permanent("invalid subject".to_owned()));
    }
    Ok(())
}

/// Postmark (`POST https://api.postmarkapp.com/email`, header `X-Postmark-Server-Token`).
pub struct PostmarkMailer {
    http: ProviderHttp,
    server_token: Secret,
    from: String,
    message_stream: String,
    url: String,
}

impl PostmarkMailer {
    pub const API_URL: &'static str = "https://api.postmarkapp.com/email";

    pub fn new(http: ProviderHttp, server_token: Secret, from: String) -> Self {
        Self::with_url(http, server_token, from, Self::API_URL.to_owned())
    }

    /// Another endpoint (tests use a local stand-in).
    pub fn with_url(http: ProviderHttp, server_token: Secret, from: String, url: String) -> Self {
        Self {
            http,
            server_token,
            from,
            message_stream: "outbound".to_owned(),
            url,
        }
    }
}

impl Mailer for PostmarkMailer {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a> {
        Box::pin(async move {
            check_message(&self.from, email)?;
            let body = serde_json::json!({
                "From": self.from,
                "To": email.to,
                "Subject": email.subject,
                "TextBody": email.text,
                "MessageStream": self.message_stream,
            });
            let response = self
                .http
                .post_json(
                    &self.url,
                    &[("X-Postmark-Server-Token", self.server_token.expose())],
                    body.to_string().into_bytes(),
                    16 * 1024,
                )
                .await
                .map_err(MailError::Temporary)?;
            let status = response.status.as_u16();
            match status {
                200..=299 => Ok(()),
                429 | 500..=599 => Err(MailError::Temporary(format!("postmark status {status}"))),
                // 401/422: bad token, invalid address, inactive recipient: retrying won't help.
                _ => Err(MailError::Permanent(format!("postmark status {status}"))),
            }
        })
    }
}

/// Keeps sent messages in memory (tests).
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingMailer {
    pub sent: std::sync::Mutex<Vec<Email>>,
}

#[cfg(any(test, feature = "test-support"))]
impl Mailer for RecordingMailer {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a> {
        Box::pin(async move {
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(email.clone());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn addresses_cannot_inject_headers() {
        assert!(valid_address("cat@example.com"));
        assert!(!valid_address("cat@example.com\r\nBcc: x@y.z"));
        assert!(!valid_address("cat <cat@example.com>"));
        assert!(!valid_address("no-at-sign"));
        assert!(!valid_address("a@b"));
        assert!(!valid_address("a@@b.com"));
    }

    #[test]
    fn dot_stuffing_escapes_leading_dots() {
        assert_eq!(dot_stuff("a\n.b\n..c"), "a\r\n..b\r\n...c");
    }

    #[test]
    fn debug_never_prints_the_body() {
        let email = Email {
            to: "cat@example.com".into(),
            subject: "Sign in".into(),
            text: "secret-link".into(),
        };
        assert!(!format!("{email:?}").contains("secret-link"));
        assert!(!format!("{email:?}").contains("cat@example.com"));
    }

    /// A tiny SMTP server that accepts one message and returns its DATA section.
    async fn fake_smtp() -> (u16, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut read = BufReader::new(read);
            write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
            let mut data = String::new();
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if read.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        in_data = false;
                        write.write_all(b"250 queued\r\n").await.unwrap();
                    } else {
                        data.push_str(&line);
                    }
                    continue;
                }
                let reply: &[u8] = match line.split_whitespace().next().unwrap_or_default() {
                    "EHLO" => b"250-fake\r\n250 8BITMIME\r\n",
                    "DATA" => {
                        in_data = true;
                        b"354 go\r\n"
                    }
                    "QUIT" => break,
                    _ => b"250 ok\r\n",
                };
                write.write_all(reply).await.unwrap();
            }
            data
        });
        (port, handle)
    }

    #[tokio::test]
    async fn smtp_adapter_delivers_a_message() {
        let (port, server) = fake_smtp().await;
        let mailer = SmtpMailer::new("127.0.0.1".into(), port, "no-reply@example.com".into());
        let email = Email {
            to: "cat@example.com".into(),
            subject: "Sign in".into(),
            text: "Hello\n.hidden line".into(),
        };
        mailer.send(&email).await.unwrap();
        let data = server.await.unwrap();
        assert!(data.contains("Subject: Sign in\r\n"));
        assert!(data.contains("To: <cat@example.com>"));
        assert!(data.contains("\r\n..hidden line"), "dot-stuffed: {data}");
    }

    #[tokio::test]
    async fn invalid_messages_are_refused_before_connecting() {
        let mailer = SmtpMailer::new("127.0.0.1".into(), 1, "no-reply@example.com".into());
        let email = Email {
            to: "x@example.com\r\nBcc: y@example.com".into(),
            subject: "s".into(),
            text: String::new(),
        };
        assert!(matches!(
            mailer.send(&email).await,
            Err(MailError::Permanent(_))
        ));
    }
}
