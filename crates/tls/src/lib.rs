//! One rustls configuration for every HTTP client in zero-search.
//!
//! reqwest's `rustls-tls-native-roots` backend reads the operating
//! system's root store every time a client is built. The library
//! builds about 22 clients when it starts (one or two per provider),
//! so that read happened 22 times — measured 2026-11-17 on Windows:
//! 56 ms in total, which took the MCP server's `initialize` reply
//! from 13 ms (v0.6.0) to 66 ms. Here the root store is read once per
//! process and every client gets a clone of the same
//! `rustls::ClientConfig` (cheap: the roots sit behind an `Arc`).
//!
//! Roots: the OS store, which also carries locally installed CAs (an
//! intercepting corporate proxy, for example) exactly as before. Only
//! if the OS store yields no usable certificate (a minimal container)
//! are the bundled Mozilla roots used instead of failing.
//!
//! ALPN: reqwest leaves ALPN untouched on a preconfigured config, so
//! the protocols are set here: `h2` + `http/1.1` normally, and only
//! `http/1.1` for clients built with `http1_only()` — offering `h2`
//! to a server while the client cannot speak it would break those.

use std::sync::{Arc, LazyLock};

static ROOTS: LazyLock<Arc<rustls::RootCertStore>> = LazyLock::new(|| {
    let mut roots = rustls::RootCertStore::empty();
    // Unparsable entries are skipped, as reqwest does: OS stores often
    // hold ancient or malformed certificates.
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    Arc::new(roots)
});

fn config(alpn: &[&[u8]]) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(ROOTS.clone())
        .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    config
}

static DEFAULT: LazyLock<rustls::ClientConfig> = LazyLock::new(|| config(&[b"h2", b"http/1.1"]));
static HTTP1: LazyLock<rustls::ClientConfig> = LazyLock::new(|| config(&[b"http/1.1"]));

/// The shared configuration (ALPN `h2`, `http/1.1`).
pub fn client_config() -> rustls::ClientConfig {
    DEFAULT.clone()
}

/// The shared configuration for HTTP/1.1-only clients (ALPN `http/1.1`).
pub fn client_config_http1() -> rustls::ClientConfig {
    HTTP1.clone()
}

/// Apply the shared TLS configuration to a reqwest builder.
pub trait SharedTls: Sized {
    /// For clients that may use HTTP/2.
    fn shared_tls(self) -> Self;
    /// For clients built with `http1_only()`.
    fn shared_tls_http1(self) -> Self;
}

impl SharedTls for reqwest::ClientBuilder {
    fn shared_tls(self) -> Self {
        self.use_preconfigured_tls(client_config())
    }
    fn shared_tls_http1(self) -> Self {
        self.use_preconfigured_tls(client_config_http1())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_share_one_root_store_and_differ_in_alpn() {
        let a = client_config();
        let b = client_config_http1();
        assert_eq!(a.alpn_protocols, vec![b"h2".to_vec(), b"http/1.1".to_vec()]);
        assert_eq!(b.alpn_protocols, vec![b"http/1.1".to_vec()]);
        assert!(!ROOTS.is_empty());
    }

    #[test]
    fn building_many_clients_is_cheap_after_the_first() {
        let _ = reqwest::Client::builder().shared_tls().build().unwrap();
        let t = std::time::Instant::now();
        for _ in 0..22 {
            let _ = reqwest::Client::builder().shared_tls().build().unwrap();
        }
        // Generous bound (debug build, loaded CI runners); per-client
        // OS store loads cost ~2.5 ms each in release.
        assert!(t.elapsed() < std::time::Duration::from_millis(500));
    }
}
