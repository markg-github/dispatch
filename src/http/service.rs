use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{stream, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::{Method, StatusCode as Code};
use hyper::{Request, Response};
use reqwest::Client;
use std::convert::Infallible;
use tokio::sync::Mutex;
use tracing::{debug, info, warn, error};

use crate::github::{Asset, GitHub, Report, Type};
use crate::tui::Status;

// This contains the bytes of the poweroff.efi module.
const POWEROFF_EFI: &[u8] = include_bytes!(env!("POWEROFF_BIN_PATH"));
const EMPTY: &[u8] = &[];

/// Main HTTP service that handles all requests
#[derive(Debug)]
pub struct Service {
    remote: IpAddr,
    status: Arc<Mutex<Status>>,
    github: Arc<GitHub>,
    client: Client,
    path: Arc<String>,
}

impl Service {
    pub const fn new(
        remote: IpAddr,
        status: Arc<Mutex<Status>>,
        github: Arc<GitHub>,
        client: Client,
        path: Arc<String>,
    ) -> Self {
        Self {
            remote,
            status,
            github,
            client,
            path,
        }
    }
}

impl hyper::service::Service<Request<Incoming>> for Service {
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = anyhow::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    // #[tracing::instrument(level = "info", name = "http_request", skip(self, req), fields(ip = %self.remote, method = %req.method(), path = %req.uri().path()))]
    #[tracing::instrument(level = "info", 
                            name = "http_request", 
                            skip(self, req))]
    fn call(&self, req: Request<Incoming>) -> Self::Future {


        // 1. Get the current span created by the macro
        let span = tracing::Span::current();

        // 2. Create the owned values 
        //    (The .clone() and .to_string() methods must be called here)
        let ip = self.remote.clone(); 
        let method = req.method().clone();
        let path = req.uri().path().to_string(); // Use .to_string() for the owned String

        // 3. Record the owned values onto the span
        //    Note: The `req` is passed by value, so we must record fields before awaiting.
        span.record("ip", &tracing::field::display(ip));
        span.record("method", &tracing::field::display(method));
        span.record("path", &tracing::field::display(path));






        let status = self.status.clone();
        let client = self.client.clone();
        let github = self.github.clone();
        let path = self.path.clone();
        let remote = self.remote;

        Box::pin(async move {
            if req.uri().path() != *path {
                warn!(ip = %remote, requested_path = %req.uri().path(), expected_path = %*path, "path mismatch - returning 404");
                return Ok(EMPTY.reply(Code::NOT_FOUND, None, None));
            }

            let (response, ct) = match *req.method() {
                // The POST request is used to signal the start of a job.
                Method::POST => {
                    tracing::info!(ip = %remote, "received boot beacon");
                    if status.lock().await.update().booting(remote) {
                        tracing::info!(ip = %remote, "boot beacon accepted");
                        return Ok(EMPTY.reply(None, None, None));
                    }

                    tracing::warn!(ip = %remote, "boot beacon rejected - no downloading job for this IP");
                    return Ok(EMPTY.reply(Code::EXPECTATION_FAILED, None, None));
                }

                // The PUT request is used to report completion of a job.
                Method::PUT => {
                    tracing::info!(ip = %remote, "received job report");
                    // Collect the request body
                    let bytes = req.into_body().collect().await?.to_bytes();
                    let report: Report = match serde_json::from_slice(&bytes) {
                        Err(..) => {
                            tracing::warn!(ip = %remote, "invalid report payload");
                            return Ok(EMPTY.reply(Code::BAD_REQUEST, None, None));
                        }
                        Ok(report_value) => {
                            let report: Report = report_value;
                            // tracing::info!(ip = %remote, title = %report.title, "parsed report");
                            tracing::info!(ip = %remote, "parsed report");
                            report
                        }
                    };

                    // Display that the job has been reported.
                    if !status.lock().await.update().report(remote) {
                        tracing::warn!(ip = %remote, "report rejected - no booting job for this IP");
                        return Ok(EMPTY.reply(Code::EXPECTATION_FAILED, None, None));
                    }

                    // Create a GitHub issue for the report.
                    let reported = tokio::time::Instant::now();
                    if github.report(report).await.is_err() {
                        tracing::error!(ip = %remote, "failed to create GitHub issue for report");
                        return Ok(EMPTY.reply(Code::INTERNAL_SERVER_ERROR, None, None));
                    }

                    // Mark the job as finished.
                    tokio::spawn(async move {
                        tokio::time::sleep_until(reported + Duration::from_secs(5)).await;
                        status.lock().await.update().finish(remote);
                    });

                    tracing::info!(ip = %remote, "report accepted and GitHub issue created");
                    return Ok(EMPTY.reply(None, None, None));
                }

                // The HEAD request is used to get information about the assigned asset.
                Method::HEAD => {
                    tracing::info!(ip = %remote, "HEAD request - checking for assignment");
                    match status.clone().assign(remote).await {
                        // No asset assigned, return poweroff EFI binary.
                        None => {
                            tracing::info!(ip = %remote, "no asset assigned - serving poweroff");
                            return Ok(POWEROFF_EFI.reply(None, Type::Efi, EMPTY));
                        }

                        // Send the request (possibly redirecting...)
                        Some(asset) => {
                            tracing::info!(ip = %remote, asset = %asset.name, size = asset.size, url = %asset.url, "HEAD: proxying to upstream");
                            match client.head(asset.url).send().await {
                                Ok(resp) => {
                                    let status_code = resp.status();
                                    let content_length = resp.headers().get("content-length").and_then(|v| v.to_str().ok());
                                    tracing::info!(ip = %remote, upstream_status = %status_code, content_length = ?content_length, "HEAD: upstream response received");
                                    (resp, asset.mime)
                                }
                                Err(e) => {
                                    tracing::error!(ip = %remote, %e, "HEAD: upstream request failed");
                                    return Ok(EMPTY.reply(Code::BAD_GATEWAY, None, None));
                                }
                            }
                        }
                    }
                }

                // The GET request is used to fetch the assigned asset.
                Method::GET => {
                    tracing::info!(ip = %remote, "GET request - checking for assignment");
                    match status.clone().assign(remote).await {
                        // No asset assigned, return poweroff EFI binary.
                        None => {
                            tracing::info!(ip = %remote, "no asset assigned - serving poweroff");
                            return Ok(POWEROFF_EFI.reply(None, Type::Efi, None));
                        }

                        // Send the request (possibly redirecting...)
                        Some(asset) => {
                            tracing::info!(ip = %remote, asset = %asset.name, size = asset.size, url = %asset.url, "GET: proxying to upstream");
                            match client.get(asset.url).send().await {
                                Ok(resp) => {
                                    let status_code = resp.status();
                                    let content_length = resp.headers().get("content-length").and_then(|v| v.to_str().ok());
                                    tracing::info!(ip = %remote, upstream_status = %status_code, content_length = ?content_length, "GET: upstream response received");
                                    status.lock().await.update().downloading(remote);
                                    (resp, asset.mime)
                                }
                                Err(e) => {
                                    tracing::error!(ip = %remote, %e, "GET: upstream request failed");
                                    return Ok(EMPTY.reply(Code::BAD_GATEWAY, None, None));
                                }
                            }
                        }
                    }
                }

                // Bad method.
                _ => {
                    return Ok(Response::builder()
                        .status(Code::METHOD_NOT_ALLOWED)
                        .header("allow", "GET, POST, HEAD, PUT")
                        .body(EMPTY.embody())?)
                }
            };

            let content_type = ct.content_type().parse().unwrap();

            // Construct the response.
            let mut builder = Response::builder().status(response.status());
            for (key, mut value) in response.headers() {
                // GitHub always returns `application/octet-stream` for EFI
                // binaries, so we override it here.
                if key == "content-type" {
                    value = &content_type;
                }

                builder = builder.header(key, value);
            }

            // Stream the response body directly, mapping errors to Infallible
            let remote_for_stream = remote;
            let mut total_bytes: u64 = 0;
            Ok(builder.body(BoxBody::new(StreamBody::new(Box::pin(
                response.bytes_stream().map(move |result| {
                    match result {
                        Ok(bytes) => {
                            total_bytes += bytes.len() as u64;
                            // Log first chunk and periodically (every 10MB)
                            if total_bytes <= 1024 * 1024 || total_bytes % (10 * 1024 * 1024) < bytes.len() as u64 {
                                debug!(ip = %remote_for_stream, chunk_bytes = bytes.len(), total_bytes = total_bytes, "streaming chunk");
                            }
                            Ok(Frame::data(bytes))
                        }
                        Err(e) => {
                            error!(ip = %remote_for_stream, %e, bytes_streamed = total_bytes, "stream error");
                            Ok(Frame::data(Bytes::new()))
                        }
                    }
                }),
            ))))?)
        })
    }
}

trait Embody {
    fn embody(self) -> BoxBody<Bytes, Infallible>;
}

impl Embody for &'static [u8] {
    fn embody(self) -> BoxBody<Bytes, Infallible> {
        BoxBody::new(StreamBody::new(Box::pin(stream::once(async move {
            Ok(Frame::data(Bytes::from(self)))
        }))))
    }
}

trait Reply {
    fn reply(
        self,
        code: impl Into<Option<Code>>,
        ct: impl Into<Option<Type>>,
        body: impl Into<Option<&'static [u8]>>,
    ) -> Response<BoxBody<Bytes, Infallible>>;
}

impl Reply for &'static [u8] {
    fn reply(
        self,
        code: impl Into<Option<Code>>,
        ct: impl Into<Option<Type>>,
        body: impl Into<Option<&'static [u8]>>,
    ) -> Response<BoxBody<Bytes, Infallible>> {
        let mut builder = Response::builder()
            .status(code.into().unwrap_or(Code::OK))
            .header("content-length", self.len());

        if let Some(ct) = ct.into() {
            builder = builder.header("content-type", ct.content_type());
        }

        builder.body(body.into().unwrap_or(self).embody()).unwrap()
    }
}

trait Assign {
    async fn assign(self, ip: IpAddr) -> Option<Asset>;
}

impl Assign for Arc<Mutex<Status>> {
    async fn assign(self, ip: IpAddr) -> Option<Asset> {
        self.lock().await.update().assign(ip)
    }
}
