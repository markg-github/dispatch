use std::sync::Arc;

use hyper::server::conn::http1::Builder;
use hyper_util::rt::TokioIo;
use reqwest::redirect::Policy;
use reqwest::Client;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::service::Service;
use crate::github::GitHub;
use crate::tui::Status;

pub struct Server {
    listener: TcpListener,
    status: Arc<Mutex<Status>>,
    github: Arc<GitHub>,
    client: Client,
    path: Arc<String>,
}

impl Server {
    const REDIRECTS: usize = 2;
    const DOMAINS: &[&str] = &["githubusercontent.com"];

    pub fn new(
        listener: TcpListener,
        status: Arc<Mutex<Status>>,
        github: Arc<GitHub>,
        path: Arc<String>,
    ) -> reqwest::Result<Self> {
        let policy = Policy::custom(move |attempt| {
            if attempt.previous().len() > Self::REDIRECTS {
                return attempt.stop();
            }

            let Some(host) = attempt.url().host_str() else {
                return attempt.stop();
            };

            for domain in Self::DOMAINS {
                if let Some(prefix) = host.strip_suffix(domain) {
                    if prefix.is_empty() || prefix.ends_with('.') {
                        return attempt.follow();
                    }
                }
            }

            attempt.stop()
        });

        if let Some(token) = github.token() {
            let token_preview = if token.len() > 8 { &token[..8] } else { token };
            tracing::info!(token_present = true, token_preview = token_preview, "Adding GitHub token to client");
        } else {
            tracing::warn!("No GitHub token available");
        }

        // Build client with GitHub authentication if token is available
        let mut client_builder = Client::builder().redirect(policy);
        
        if let Some(token) = github.token() {
            tracing::info!(token_present = true, token_prefix = &token[..token.len().min(8)], "GitHub token found, adding Authorization header");
            let mut headers = reqwest::header::HeaderMap::new();
            if true {
                headers.insert("Authorization", format!("token {token}").parse().unwrap());

            }
            else {
                headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());

            }
            headers.insert("User-Agent", concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")).parse().unwrap());
            tracing::debug!(?headers, "Building reqwest client with headers");
            client_builder = client_builder.default_headers(headers);
        }
        else {
            tracing::warn!("No GitHub token found, proceeding without Authorization header");
        }


        Ok(Self {
            listener,
            status,
            github,
            client: client_builder.build()?,
            path,
        })
    }

    pub async fn serve(self) -> std::io::Result<()> {
        loop {
            // Accept a new connection.
            let (stream, addr) = self.listener.accept().await?;
            let status = self.status.clone();
            let github = self.github.clone();
            let client = self.client.clone();
            let path = self.path.clone();

            // Spawn a new task to handle the connection.
            tokio::spawn(async move {
                let stream = TokioIo::new(stream);
                let service = Service::new(addr.ip(), status, github, client, path);
                Builder::new().serve_connection(stream, service).await
            });
        }
    }
}
