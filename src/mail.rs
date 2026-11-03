//! Outgoing mail. Odin only sends transactional plain-text messages, and only
//! to people an administrator has added, so this stays a thin wrapper around
//! a blocking SMTP transport; callers run it on a blocking thread.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lettre::message::{Mailbox, Message, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};

use crate::config::{SmtpSecurity, SmtpSettings};

const SMTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Whatever delivers a finished message; the SMTP transport in production,
/// a recorder in tests.
pub trait MailSender: Send + Sync {
    fn send(&self, message: &Message) -> Result<()>;
}

impl MailSender for SmtpTransport {
    fn send(&self, message: &Message) -> Result<()> {
        Transport::send(self, message)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Mailer {
    sender: Arc<dyn MailSender>,
    from: Mailbox,
}

impl Mailer {
    pub fn smtp(settings: &SmtpSettings) -> Result<Self> {
        let builder = match settings.security {
            SmtpSecurity::StartTls => SmtpTransport::starttls_relay(&settings.host),
            SmtpSecurity::Tls => SmtpTransport::relay(&settings.host),
            SmtpSecurity::None => Ok(SmtpTransport::builder_dangerous(&settings.host)),
        }
        .with_context(|| format!("could not prepare a TLS connection to {}", settings.host))?
        .port(settings.port)
        .timeout(Some(SMTP_TIMEOUT));
        let builder = match &settings.credentials {
            Some((username, password)) => {
                builder.credentials(Credentials::new(username.clone(), password.clone()))
            }
            None => builder,
        };
        Ok(Self::with_sender(
            Arc::new(builder.build()),
            settings.from.clone(),
        ))
    }

    pub fn with_sender(sender: Arc<dyn MailSender>, from: Mailbox) -> Self {
        Self { sender, from }
    }

    /// Tells `to` how to choose a password. `link` is the full one-time URL.
    pub fn send_invitation(&self, to: &str, link: &str, valid_for: chrono::Duration) -> Result<()> {
        let hours = valid_for.num_hours();
        let body = format!(
            "You have been invited to Odin.\n\n\
             Open this link to choose your password and sign in:\n\n{link}\n\n\
             The link works once and expires in {hours} hours.\n\
             If you were not expecting this message, you can ignore it.\n"
        );
        let message = Message::builder()
            .from(self.from.clone())
            .to(to
                .parse::<Mailbox>()
                .with_context(|| format!("'{to}' is not a valid recipient"))?)
            .subject("You're invited to Odin")
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .context("could not build the invitation message")?;
        self.sender.send(&message)
    }
}

#[cfg(test)]
pub use tests::RecordingSender;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Keeps every message as its raw RFC 5322 text; can be told to fail.
    #[derive(Default)]
    pub struct RecordingSender {
        pub sent: Mutex<Vec<String>>,
        pub fail: std::sync::atomic::AtomicBool,
    }

    impl RecordingSender {
        /// The invitation token in the most recent message's link.
        pub fn last_token(&self) -> String {
            let sent = self.sent.lock().unwrap();
            // Long lines are quoted-printable encoded: undo soft breaks and `=`.
            let text = sent
                .last()
                .unwrap()
                .replace("=\r\n", "")
                .replace("=3D", "=");
            let start = text.find("#token=").expect("no link in the message") + "#token=".len();
            text[start..start + 64].to_string()
        }
    }

    impl MailSender for RecordingSender {
        fn send(&self, message: &Message) -> Result<()> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("connection refused");
            }
            self.sent
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&message.formatted()).into_owned());
            Ok(())
        }
    }

    fn mailer(sender: &Arc<RecordingSender>) -> Mailer {
        Mailer::with_sender(sender.clone(), "Odin <odin@example.com>".parse().unwrap())
    }

    #[test]
    fn invitation_goes_to_the_recipient_with_the_link() {
        let sender = Arc::new(RecordingSender::default());
        mailer(&sender)
            .send_invitation(
                "ada@example.com",
                "https://odin.example.com/accept-invite#token=abc",
                chrono::Duration::hours(72),
            )
            .unwrap();
        let sent = sender.sent.lock().unwrap();
        let raw = sent.first().unwrap();
        assert!(raw.contains("To: ada@example.com"));
        assert!(raw.contains("From: Odin <odin@example.com>"));
        assert!(raw.contains("Content-Type: text/plain"));
        assert!(
            raw.contains("https://odin.example.com/accept-invite#token=3Dabc")
                || raw.contains("https://odin.example.com/accept-invite#token=abc")
        );
        assert!(raw.contains("72 hours"));
    }

    #[test]
    fn an_invalid_recipient_is_refused_before_sending() {
        let sender = Arc::new(RecordingSender::default());
        let result = mailer(&sender).send_invitation(
            "not an address",
            "https://odin.example.com/",
            chrono::Duration::hours(72),
        );
        assert!(result.is_err());
        assert!(sender.sent.lock().unwrap().is_empty());
    }

    /// A one-message SMTP server on a loopback port; returns the DATA it received.
    fn fake_smtp_server() -> (u16, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut say = |line: &str| writer.write_all(format!("{line}\r\n").as_bytes()).unwrap();
            say("220 fake ESMTP");
            let (mut data, mut in_data) = (String::new(), false);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        in_data = false;
                        say("250 queued");
                    } else {
                        data.push_str(&line);
                    }
                    continue;
                }
                let command = line.trim_end().to_ascii_uppercase();
                if command.starts_with("EHLO") {
                    say("250 fake");
                } else if command == "DATA" {
                    in_data = true;
                    say("354 go ahead");
                } else if command == "QUIT" {
                    say("221 bye");
                    break;
                } else {
                    say("250 ok");
                }
            }
            data
        });
        (port, handle)
    }

    #[test]
    fn an_invitation_travels_over_smtp() {
        let (port, server) = fake_smtp_server();
        let mailer = Mailer::smtp(&SmtpSettings {
            host: "127.0.0.1".into(),
            port,
            security: SmtpSecurity::None,
            credentials: None,
            from: "Odin <odin@example.com>".parse().unwrap(),
        })
        .unwrap();
        mailer
            .send_invitation(
                "ada@example.com",
                "https://odin.example.com/accept-invite#token=abc",
                chrono::Duration::hours(72),
            )
            .unwrap();
        let data = server.join().unwrap();
        assert!(data.contains("To: ada@example.com"), "{data}");
        assert!(data.contains("Subject: You're invited to Odin"), "{data}");
    }

    #[test]
    fn an_unreachable_server_is_an_error_not_a_hang() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let mailer = Mailer::smtp(&SmtpSettings {
            host: "127.0.0.1".into(),
            port,
            security: SmtpSecurity::None,
            credentials: None,
            from: "odin@example.com".parse().unwrap(),
        })
        .unwrap();
        let result = mailer.send_invitation(
            "ada@example.com",
            "https://odin.example.com/",
            chrono::Duration::hours(72),
        );
        assert!(result.is_err());
    }

    #[test]
    fn smtp_transport_builds_for_every_security_mode() {
        for security in [
            SmtpSecurity::StartTls,
            SmtpSecurity::Tls,
            SmtpSecurity::None,
        ] {
            let settings = SmtpSettings {
                host: "mail.example.com".into(),
                port: 2525,
                security,
                credentials: (security != SmtpSecurity::None).then(|| ("u".into(), "p".into())),
                from: "odin@example.com".parse().unwrap(),
            };
            Mailer::smtp(&settings).unwrap();
        }
    }
}
