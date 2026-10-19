//! QR login: the poll loop (ours, because steamroom's `poll_auth_session` drops `new_challenge_url`) and the rendering.

use std::time::Duration;

use qrcode::{render::unicode::Dense1x2, QrCode};

use crate::error::{Error, Result};

/// One poll reply, reduced to what the loop needs.
#[derive(Debug, Default, Clone)]
pub struct PollReply {
    /// Steam moved the session to a new client id (the QR code changed).
    pub new_client_id: Option<u64>,
    /// The code to show now, if it changed.
    pub new_challenge_url: Option<String>,
    /// Set once the user approved the login.
    pub refresh_token: Option<String>,
    /// Set together with the token.
    pub account_name: Option<String>,
}

/// The outcome of an approved QR login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrTokens {
    /// The refresh token.
    pub refresh_token: String,
    /// The account name Steam reported (may be empty).
    pub account_name: String,
}

/// Something that polls `PollAuthSessionStatus` for a client id.
pub trait Poller {
    /// One poll.
    fn poll(&mut self, client_id: u64) -> impl std::future::Future<Output = Result<PollReply>>;
}

/// Polls until the token arrives. `show` is called with the first URL and again for every new one.
pub async fn run(
    poller: &mut impl Poller,
    mut client_id: u64,
    url: &str,
    interval: Duration,
    mut show: impl FnMut(&str),
) -> Result<QrTokens> {
    show(url);
    loop {
        tokio::time::sleep(interval).await;
        let reply = poller.poll(client_id).await?;
        if let Some(id) = reply.new_client_id {
            client_id = id;
        }
        if let Some(token) = reply.refresh_token.filter(|t| !t.is_empty()) {
            return Ok(QrTokens {
                refresh_token: token,
                account_name: reply.account_name.unwrap_or_default(),
            });
        }
        if let Some(new_url) = reply.new_challenge_url {
            show(&new_url);
        }
    }
}

/// A QR code as modules, for a host that draws it itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QrMatrix {
    /// Modules per side (without a quiet zone).
    pub size: usize,
    /// Row by row, `true` for a dark module; `size * size` entries.
    pub dark: Vec<bool>,
}

/// The modules of the QR code of `url`.
pub fn matrix(url: &str) -> Result<QrMatrix> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| Error::Protocol(format!("QR code: {e}")))?;
    Ok(QrMatrix {
        size: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
    })
}

/// Renders a URL as a terminal QR code (two modules per character cell).
pub fn render(url: &str) -> Result<String> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| Error::Protocol(format!("QR code: {e}")))?;
    Ok(code.render::<Dense1x2>().quiet_zone(true).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        replies: Vec<PollReply>,
        seen_ids: Vec<u64>,
    }

    impl Poller for Fake {
        async fn poll(&mut self, client_id: u64) -> Result<PollReply> {
            self.seen_ids.push(client_id);
            Ok(self.replies.remove(0))
        }
    }

    #[test]
    fn the_matrix_is_square_and_has_both_colours() {
        let m = matrix("https://s.team/q/1/1").unwrap();
        assert_eq!(m.dark.len(), m.size * m.size);
        assert!(m.dark.contains(&true) && m.dark.contains(&false));
        // The three finder patterns start with a dark corner module.
        assert!(m.dark[0] && m.dark[m.size - 1] && m.dark[(m.size - 1) * m.size]);
    }

    #[tokio::test]
    async fn rotation_redraws_once_and_returns_the_token() {
        let mut fake = Fake {
            replies: vec![
                PollReply::default(),
                PollReply {
                    new_client_id: Some(2),
                    new_challenge_url: Some("url-2".into()),
                    ..Default::default()
                },
                PollReply::default(),
                PollReply {
                    refresh_token: Some("tok".into()),
                    account_name: Some("gordon".into()),
                    ..Default::default()
                },
            ],
            seen_ids: vec![],
        };
        let mut shown = vec![];
        let tokens = run(&mut fake, 1, "url-1", Duration::ZERO, |u| {
            shown.push(u.to_string())
        })
        .await
        .unwrap();
        assert_eq!(
            tokens,
            QrTokens {
                refresh_token: "tok".into(),
                account_name: "gordon".into()
            }
        );
        assert_eq!(shown, ["url-1", "url-2"]);
        assert_eq!(fake.seen_ids, [1, 1, 2, 2]);
    }

    #[test]
    fn renders_something() {
        assert!(render("https://s.team/q/1/2").unwrap().lines().count() > 5);
    }
}
