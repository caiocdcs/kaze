//! Email over SMTP: plain text with a small HTML version.

use std::{fmt::Write as _, time::Duration};

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{IntegrationError, Message, filled, keep_opt, mask_opt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Email {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub security: SmtpSecurity,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub username: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub password: Option<String>,
    /// `kaze@example.com` or `kaze <kaze@example.com>`.
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS (usually port 587).
    #[default]
    Starttls,
    /// TLS from the start (usually port 465).
    Tls,
    /// No encryption, for a relay on the local network.
    None,
}

impl Email {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("give the SMTP server's host name".into());
        }
        if self.port == 0 {
            return Err("give the SMTP server's port".into());
        }
        self.from
            .parse::<Mailbox>()
            .map_err(|e| format!("from address: {e}"))?;
        if self.to.is_empty() {
            return Err("add at least one recipient".into());
        }
        for to in &self.to {
            to.parse::<Mailbox>()
                .map_err(|e| format!("recipient {to}: {e}"))?;
        }
        Ok(())
    }

    pub(super) fn mask(&mut self) {
        mask_opt(&mut self.password);
    }

    pub(super) fn keep_secrets(&mut self, stored: &Self) {
        keep_opt(&mut self.password, stored.password.as_ref());
    }

    pub(super) async fn send(&self, message: &Message) -> Result<(), IntegrationError> {
        let invalid = |e: &dyn std::fmt::Display| IntegrationError::Invalid(e.to_string());
        let mut mail = lettre::Message::builder()
            .from(self.from.parse().map_err(|e| invalid(&e))?)
            .subject(&message.title);
        for to in &self.to {
            mail = mail.to(to.parse().map_err(|e| invalid(&e))?);
        }
        let mail = mail
            .multipart(MultiPart::alternative_plain_html(
                message.body.clone(),
                html(message),
            ))
            .map_err(|e| invalid(&e))?;

        let host = self.host.trim();
        let transport = match self.security {
            SmtpSecurity::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
            SmtpSecurity::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
        };
        let mut transport = transport
            .port(self.port)
            .timeout(Some(Duration::from_secs(30)));
        if let Some(username) = filled(self.username.as_deref()) {
            transport = transport.credentials(Credentials::new(
                username.to_owned(),
                self.password.clone().unwrap_or_default(),
            ));
        }
        transport.build().send(mail).await?;
        Ok(())
    }
}

fn html(message: &Message) -> String {
    let mut out = format!(
        "<!doctype html><meta charset=\"utf-8\"><h2>{}</h2><ul>",
        escape(&message.title)
    );
    for line in message.body.lines() {
        let _ = write!(out, "<li>{}</li>", escape(line));
    }
    out.push_str("</ul>");
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    use super::*;
    use crate::integrations::tests::message;

    /// A one-connection SMTP server that accepts everything except, when
    /// `reject` is given, the sender (with that reply). Returns its port
    /// and the lines the client sent.
    async fn smtp(reject: Option<&'static str>) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            write.write_all(b"220 stand-in ESMTP\r\n").await.unwrap();
            let mut in_data = false;
            while let Ok(Some(line)) = lines.next_line().await {
                log.lock().unwrap().push(line.clone());
                let verb = line.split(' ').next().unwrap_or("").to_ascii_uppercase();
                let reply = match verb.as_str() {
                    _ if in_data && line == "." => {
                        in_data = false;
                        "250 queued\r\n"
                    }
                    _ if in_data => continue,
                    "MAIL" => reject.unwrap_or("250 ok\r\n"),
                    "DATA" => {
                        in_data = true;
                        "354 go ahead\r\n"
                    }
                    "QUIT" => "221 bye\r\n",
                    _ => "250 ok\r\n",
                };
                if write.write_all(reply.as_bytes()).await.is_err() || verb == "QUIT" {
                    break;
                }
            }
        });
        (port, seen)
    }

    fn email(port: u16) -> Email {
        Email {
            host: "127.0.0.1".into(),
            port,
            security: SmtpSecurity::None,
            username: None,
            password: None,
            from: "kaze <kaze@example.com>".into(),
            to: vec!["me@example.com".into(), "you@example.com".into()],
        }
    }

    #[tokio::test]
    async fn sends_one_mail_to_every_recipient() {
        let (port, seen) = smtp(None).await;
        email(port).send(&message()).await.unwrap();
        let seen = seen.lock().unwrap();
        let has = |text: &str| seen.iter().any(|line| line.contains(text));
        assert!(has("MAIL FROM:<kaze@example.com>"), "{seen:#?}");
        assert!(has("RCPT TO:<me@example.com>"));
        assert!(has("RCPT TO:<you@example.com>"));
        assert!(has(r#"Subject: Berserk "374""#));
        assert!(has("Content-Type: text/plain"));
        assert!(has("Content-Type: text/html"));
        assert!(has("<li>line one</li><li>line two</li>"));
    }

    #[tokio::test]
    async fn rejections_retry_only_when_temporary() {
        let (port, _) = smtp(Some("550 no such sender\r\n")).await;
        let err = email(port).send(&message()).await.unwrap_err();
        assert!(!err.is_retryable(), "{err}");
        assert!(err.to_string().contains("no such sender"), "{err}");

        let (port, _) = smtp(Some("421 try later\r\n")).await;
        let err = email(port).send(&message()).await.unwrap_err();
        assert!(err.is_retryable(), "{err}");
    }

    #[test]
    fn validation() {
        assert!(email(25).validate().is_ok());
        let no_one = Email {
            to: Vec::new(),
            ..email(25)
        };
        assert!(no_one.validate().is_err());
        let bad_from = Email {
            from: "not an address".into(),
            ..email(25)
        };
        assert!(bad_from.validate().is_err());
    }

    #[test]
    fn html_is_escaped() {
        let message = Message {
            title: "<b>".into(),
            body: "a & b".into(),
            ..message()
        };
        let html = html(&message);
        assert!(html.contains("<h2>&lt;b&gt;</h2>"));
        assert!(html.contains("<li>a &amp; b</li>"));
    }
}
