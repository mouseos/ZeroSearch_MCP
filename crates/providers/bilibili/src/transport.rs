//! Unary gRPC over reqwest (HTTP/2 negotiated by ALPN).
//!
//! This used to be a hand-written HTTP/2 client (HPACK + rustls, one new
//! TCP+TLS connection per call), written because raw protobuf `-bin`
//! metadata did not fit `http::HeaderValue`. The ingress carries those
//! values base64-encoded (see [`crate::encode_metadata_wire`]), so every
//! header is printable text and an ordinary HTTP client fits. One
//! reqwest client per [`crate::BilibiliClient`] keeps its connections in
//! a pool, so later calls reuse the TLS session.
//!
//! Trailers: reqwest's `Response` has no trailer accessor, but its body
//! still yields the HTTP/2 trailer frame; collecting the body through
//! `http_body_util` returns both. A trailers-only reply (the usual shape
//! of a gRPC error) puts `grpc-status` in the response headers instead.
//!
//! Egress choice is a deployment concern of the caller, never a
//! hardcoded behaviour: the default is a direct connection (system
//! proxy settings are deliberately not consulted, as before).

use anyhow::{anyhow, Context, Result};
use bytes::{BufMut, BytesMut};
use http_body_util::BodyExt;
use prost::Message;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;
use zs_tls::SharedTls;

/// How the connection to the gRPC host is established.
#[derive(Clone, Debug)]
pub enum Egress {
    /// Direct TCP to the host.
    Direct,
    /// TCP via a SOCKS5 proxy (`host:port`, e.g. `127.0.0.1:1080`); the
    /// proxy resolves the host name, as before.
    Socks5(String),
}

/// Build the HTTP client for one egress. The client owns the
/// connection pool, so it is built once per [`crate::BilibiliClient`].
pub fn build_client(egress: &Egress) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .shared_tls()
        // gRPC compresses per message (grpc-encoding), never with an
        // HTTP content-encoding; do not advertise one.
        .no_gzip()
        .no_brotli()
        .pool_idle_timeout(Duration::from_secs(60));
    let builder = match egress {
        Egress::Direct => builder.no_proxy(),
        Egress::Socks5(proxy) => builder.proxy(
            reqwest::Proxy::all(format!("socks5h://{proxy}"))
                .with_context(|| format!("invalid socks5 proxy {proxy}"))?,
        ),
    };
    builder.build().context("build http client")
}

/// One unary gRPC call. `headers` are (name, value-bytes) pairs; every
/// value must be valid header text (`-bin` values arrive base64-encoded).
pub async fn grpc_unary<Req, Resp>(
    client: &reqwest::Client,
    host: &'static str,
    path: &'static str,
    headers: &[(String, Vec<u8>)],
    msg: &Req,
) -> Result<Resp>
where
    Req: Message,
    Resp: Message + Default,
{
    tokio::time::timeout(
        Duration::from_secs(40),
        grpc_unary_inner(client, host, path, headers, msg),
    )
    .await
    .context("grpc call timed out")?
}

async fn grpc_unary_inner<Req, Resp>(
    client: &reqwest::Client,
    host: &'static str,
    path: &'static str,
    headers: &[(String, Vec<u8>)],
    msg: &Req,
) -> Result<Resp>
where
    Req: Message,
    Resp: Message + Default,
{
    let mut map = HeaderMap::new();
    map.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc"),
    );
    map.insert(reqwest::header::TE, HeaderValue::from_static("trailers"));
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| anyhow!("invalid header name {name:?}"))?;
        let value = HeaderValue::from_bytes(value)
            .map_err(|_| anyhow!("invalid header value for {name}"))?;
        map.append(name, value);
    }

    // One gRPC length-prefixed message (uncompressed).
    let body_bytes = msg.encode_to_vec();
    let mut grpc_frame = BytesMut::with_capacity(body_bytes.len() + 5);
    grpc_frame.put_u8(0);
    grpc_frame.put_u32(body_bytes.len() as u32);
    grpc_frame.extend_from_slice(&body_bytes);

    let resp = client
        .post(format!("https://{host}{path}"))
        .headers(map)
        .body(grpc_frame.freeze())
        .send()
        .await
        .context("send grpc request")?;

    let http_status = resp.status();
    let resp_headers = resp.headers().clone();
    let collected = reqwest::Body::from(resp)
        .collect()
        .await
        .context("read grpc response")?;
    let trailers = collected.trailers().cloned().unwrap_or_default();
    let data = collected.to_bytes();

    // Trailers win over headers, as the last header block did before.
    let field = |name: &str| -> Option<String> {
        trailers
            .get(name)
            .or_else(|| resp_headers.get(name))
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
    };
    let grpc_status = field("grpc-status");
    let grpc_message = field("grpc-message");

    if http_status != reqwest::StatusCode::OK {
        let debug_headers: Vec<String> = resp_headers
            .iter()
            .chain(trailers.iter())
            .map(|(n, v)| format!("{n}: {}", String::from_utf8_lossy(v.as_bytes())))
            .collect();
        let preview: String = data
            .iter()
            .take(700)
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        return Err(anyhow!(
            "http status {}, grpc-status={grpc_status:?} grpc-message={grpc_message:?} headers={debug_headers:?} body={preview:?}",
            http_status.as_u16()
        ));
    }
    if let Some(code) = &grpc_status {
        if code != "0" {
            return Err(anyhow!(
                "grpc status {code}: {}",
                grpc_message.as_deref().unwrap_or("")
            ));
        }
    }

    decode_message(&data, &grpc_status, &grpc_message)
}

/// Decode the first length-prefixed gRPC message of a response body.
fn decode_message<Resp>(
    buf: &[u8],
    grpc_status: &Option<String>,
    grpc_message: &Option<String>,
) -> Result<Resp>
where
    Resp: Message + Default,
{
    if buf.len() < 5 {
        return Err(anyhow!(
            "grpc response too short ({} bytes), grpc-status={grpc_status:?} grpc-message={grpc_message:?}",
            buf.len()
        ));
    }
    if buf[0] != 0 {
        return Err(anyhow!("grpc response compressed (unsupported)"));
    }
    let mlen = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
    if buf.len() < 5 + mlen {
        return Err(anyhow!("grpc frame truncated"));
    }
    Resp::decode(&buf[5..5 + mlen]).context("decode grpc response")
}
