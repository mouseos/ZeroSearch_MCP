//! Slim app package: the few parts of the Goofish APK the signing
//! runtime actually uses, fetched with HTTP range requests instead
//! of downloading the whole ~269 MB package.
//!
//! The signing runtime needs (a) the SecurityGuard native libraries
//! (`lib/arm64-v8a/libsg*`, ~5 MB uncompressed) and (b) a package at
//! the app's install path from which SecurityGuard reads the app's
//! signing certificate. Neither needs the dex files, resources or
//! assets that make up almost all of the package, so this module
//! reads the ZIP central directory from the end of the remote file,
//! then fetches only the wanted entries' local records and the APK
//! Signing Block, and writes them out as a small, valid ZIP — same
//! entry bytes, same signing block, rebuilt central directory.
//!
//! Integrity: every kept entry's CRC-32 (from the central directory)
//! is checked after decompression, and the main SecurityGuard
//! library's SHA-256 is pinned by the caller.

use sha2::{Digest, Sha256};
use std::io::Read;

/// Source of byte ranges of the remote (or local) package.
pub(crate) trait RangeSource {
    /// Total size of the package in bytes.
    fn len(&self) -> Result<u64, String>;
    /// Exactly `len` bytes starting at `start`.
    fn read_at(&mut self, start: u64, len: u64) -> Result<Vec<u8>, String>;
}

/// Client addresses in mainland China sent as the "client" in the
/// edge lookups below (Shanghai Telecom, Beijing Unicom, Guangdong
/// Telecom). The CDN's DNS picks edges by the client's location;
/// asking as a mainland client yields mainland edges.
const MAINLAND_CLIENT_IPS: [&str; 3] = ["101.226.0.1", "61.135.0.1", "113.108.0.1"];

/// At most this many mainland edges are tried before falling back
/// to the system resolver.
const MAX_EDGE_CANDIDATES: usize = 8;

/// HTTP range reads (`Range: bytes=a-b`, answered with 206).
///
/// The package host is a CDN whose edges differ in which ranges they
/// can serve: measured 2026-11-17 from a Japanese line, the edge the
/// system resolver returned reset every range it had not cached
/// (most of the package, including the SecurityGuard libraries),
/// while some mainland-China edges served the same ranges, others
/// reset them too or answered 302. So reads go first to mainland
/// edges, looked up as a mainland client (Tencent's HTTPDNS, then
/// DNS-over-HTTPS with an EDNS client subnet as a second source), one
/// edge at a time until one serves the range, and only then through
/// the system resolver. No edge address is hard-coded. TLS is still
/// verified for the package host's name, and every piece used is
/// checked against a pinned SHA-256 by the caller, so the edge choice
/// cannot change what is accepted.
pub(crate) struct HttpRanges {
    url: String,
    size: u64,
    /// Remaining connection targets, in order: mainland edges, then
    /// `None` (the system resolver). The first entry is the current
    /// one; a target that fails a read is dropped.
    targets: Vec<Option<std::net::IpAddr>>,
    client: Option<reqwest::blocking::Client>,
    /// Bytes fetched so far (for the log line).
    pub fetched: u64,
}

fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
}

/// Parse a Tencent HTTPDNS answer (`ip;ip;...` optionally followed by
/// `,ttl`).
fn parse_httpdns(body: &str) -> Vec<std::net::IpAddr> {
    body.trim()
        .split(',')
        .next()
        .unwrap_or("")
        .split(';')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

/// Parse the A records out of a DNS-over-HTTPS JSON answer.
fn parse_doh_json(body: &str) -> Vec<std::net::IpAddr> {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    doc.get("Answer")
        .and_then(|a| a.as_array())
        .map(|answers| {
            answers
                .iter()
                .filter(|a| a.get("type").and_then(|t| t.as_u64()) == Some(1))
                .filter_map(|a| a.get("data").and_then(|d| d.as_str()))
                .filter_map(|d| d.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Mainland-China edges for `host`, looked up as mainland clients.
/// Lookup failures just yield fewer candidates.
fn mainland_edges(host: &str) -> Vec<std::net::IpAddr> {
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    else {
        return Vec::new();
    };
    let get = |url: String| -> Option<String> {
        client
            .get(url)
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.text().ok())
    };
    let mut edges: Vec<std::net::IpAddr> = Vec::new();
    let mut push = |found: Vec<std::net::IpAddr>| {
        for ip in found {
            if !edges.contains(&ip) {
                edges.push(ip);
            }
        }
    };
    for client_ip in MAINLAND_CLIENT_IPS {
        if let Some(body) = get(format!("http://119.29.29.29/d?dn={host}&ip={client_ip}")) {
            push(parse_httpdns(&body));
        }
    }
    for client_ip in MAINLAND_CLIENT_IPS {
        if let Some(body) = get(format!(
            "https://dns.google/resolve?name={host}&type=A&edns_client_subnet={client_ip}/24"
        )) {
            push(parse_doh_json(&body));
        }
    }
    edges.truncate(MAX_EDGE_CANDIDATES);
    edges
}

impl HttpRanges {
    pub(crate) fn new(url: &str, size: u64) -> Result<Self, String> {
        let host = url_host(url).ok_or_else(|| format!("bad package URL: {url}"))?;
        let mut targets: Vec<Option<std::net::IpAddr>> =
            mainland_edges(&host).into_iter().map(Some).collect();
        targets.push(None);
        Ok(Self {
            url: url.to_string(),
            size,
            targets,
            client: None,
            fetched: 0,
        })
    }

    /// The client for the current target, built on first use.
    fn client(&mut self) -> Result<reqwest::blocking::Client, String> {
        if let Some(c) = &self.client {
            return Ok(c.clone());
        }
        let mut builder = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(300))
            // A redirect means this edge does not serve the object;
            // the next target is tried instead.
            .redirect(reqwest::redirect::Policy::none());
        if let (Some(Some(ip)), Some(host)) = (self.targets.first(), url_host(&self.url)) {
            builder = builder.resolve(&host, std::net::SocketAddr::new(*ip, 443));
        }
        let client = builder
            .build()
            .map_err(|e| format!("cannot build the download client: {e}"))?;
        self.client = Some(client.clone());
        Ok(client)
    }

    fn fetch_once(&mut self, start: u64, len: u64) -> Result<Vec<u8>, String> {
        let end = start + len - 1;
        let client = self.client()?;
        let mut resp = client
            .get(&self.url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .map_err(|e| format!("range request {start}-{end} failed: {e}"))?;
        let status = resp.status().as_u16();
        if status != 206 {
            return Err(format!(
                "range request {start}-{end}: HTTP {status} (the package host must answer range requests)"
            ));
        }
        let mut body = Vec::with_capacity(len as usize);
        resp.read_to_end(&mut body)
            .map_err(|e| format!("range request {start}-{end} failed mid-stream: {e}"))?;
        if body.len() as u64 != len {
            return Err(format!(
                "range request {start}-{end}: got {} bytes, expected {len}",
                body.len()
            ));
        }
        Ok(body)
    }
}

impl RangeSource for HttpRanges {
    fn len(&self) -> Result<u64, String> {
        Ok(self.size)
    }

    fn read_at(&mut self, start: u64, len: u64) -> Result<Vec<u8>, String> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut last = String::new();
        loop {
            // Two tries per target; the system resolver (the last
            // target) gets two more with a pause, as before.
            let tries = if self.targets.len() == 1 { 4 } else { 2 };
            for attempt in 0..tries {
                if attempt > 1 {
                    std::thread::sleep(std::time::Duration::from_secs(2 * attempt));
                }
                match self.fetch_once(start, len) {
                    Ok(body) => {
                        self.fetched += len;
                        return Ok(body);
                    }
                    Err(e) => last = e,
                }
            }
            if self.targets.len() <= 1 {
                return Err(last);
            }
            self.targets.remove(0);
            self.client = None;
        }
    }
}

/// Range reads from a local copy of the full package (a runtime
/// assembled by an earlier version that downloaded all of it).
pub(crate) struct FileRanges(pub std::fs::File);

impl RangeSource for FileRanges {
    fn len(&self) -> Result<u64, String> {
        self.0
            .metadata()
            .map(|m| m.len())
            .map_err(|e| format!("cannot stat the app package: {e}"))
    }

    fn read_at(&mut self, start: u64, len: u64) -> Result<Vec<u8>, String> {
        use std::io::{Seek, SeekFrom};
        let mut buf = vec![0u8; len as usize];
        self.0
            .seek(SeekFrom::Start(start))
            .and_then(|_| self.0.read_exact(&mut buf))
            .map_err(|e| format!("cannot read the app package: {e}"))?;
        Ok(buf)
    }
}

/// One central-directory record.
#[derive(Debug, Clone)]
pub(crate) struct CdEntry {
    pub name: String,
    pub version_made: u16,
    pub version_needed: u16,
    pub flags: u16,
    pub method: u16,
    pub time: u16,
    pub date: u16,
    pub crc: u32,
    pub comp_size: u64,
    pub size: u64,
    pub local_offset: u64,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Parse a central directory blob.
pub(crate) fn parse_central_directory(cd: &[u8]) -> Vec<CdEntry> {
    let mut entries = Vec::new();
    let mut i = 0usize;
    while i + 46 <= cd.len() && cd[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] {
        let name_len = u16_at(cd, i + 28) as usize;
        let extra_len = u16_at(cd, i + 30) as usize;
        let comment_len = u16_at(cd, i + 32) as usize;
        if i + 46 + name_len > cd.len() {
            break;
        }
        entries.push(CdEntry {
            name: String::from_utf8_lossy(&cd[i + 46..i + 46 + name_len]).into_owned(),
            version_made: u16_at(cd, i + 4),
            version_needed: u16_at(cd, i + 6),
            flags: u16_at(cd, i + 8),
            method: u16_at(cd, i + 10),
            time: u16_at(cd, i + 12),
            date: u16_at(cd, i + 14),
            crc: u32_at(cd, i + 16),
            comp_size: u32_at(cd, i + 20) as u64,
            size: u32_at(cd, i + 24) as u64,
            local_offset: u32_at(cd, i + 42) as u64,
        });
        i += 46 + name_len + extra_len + comment_len;
    }
    entries
}

/// Locate the central directory: (offset, size).
fn find_central_directory(src: &mut dyn RangeSource) -> Result<(u64, u64), String> {
    let len = src.len()?;
    let tail_len = len.min(65_557);
    if tail_len < 22 {
        return Err("the app package is too small to be a ZIP".to_string());
    }
    let tail = src.read_at(len - tail_len, tail_len)?;
    let eocd = (0..tail.len() - 21)
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("the app package has no ZIP end-of-central-directory")?;
    Ok((
        u32_at(&tail, eocd + 16) as u64,
        u32_at(&tail, eocd + 12) as u64,
    ))
}

/// The APK Signing Block immediately before the central directory,
/// if the package has one (`APK Sig Block 42` magic).
fn read_signing_block(src: &mut dyn RangeSource, cd_offset: u64) -> Result<Vec<u8>, String> {
    if cd_offset < 32 {
        return Ok(Vec::new());
    }
    let footer = src.read_at(cd_offset - 24, 24)?;
    if &footer[8..24] != b"APK Sig Block 42" {
        return Ok(Vec::new());
    }
    let size = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    let total = size + 8;
    if total > cd_offset || total > 64 * 1024 * 1024 {
        return Err("the app package has a malformed signing block".to_string());
    }
    src.read_at(cd_offset - total, total)
}

/// Decompress one entry's stored data.
pub(crate) fn inflate(entry: &CdEntry, data: &[u8]) -> Result<Vec<u8>, String> {
    match entry.method {
        0 => Ok(data.to_vec()),
        8 => {
            let mut out = Vec::with_capacity(entry.size as usize);
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .map_err(|e| format!("cannot inflate {}: {e}", entry.name))?;
            Ok(out)
        }
        other => Err(format!(
            "{} uses unsupported ZIP method {other}",
            entry.name
        )),
    }
}

/// A kept entry: its central record plus raw (compressed) data.
pub(crate) struct KeptEntry {
    pub entry: CdEntry,
    pub data: Vec<u8>,
    /// Decompressed bytes (kept for the caller to extract).
    pub plain: Vec<u8>,
}

/// Fetch the entries `want` selects, verifying each one's CRC-32,
/// plus the signing block. Returns (entries, signing block).
pub(crate) fn fetch_entries(
    src: &mut dyn RangeSource,
    want: &dyn Fn(&str) -> bool,
) -> Result<(Vec<KeptEntry>, Vec<u8>), String> {
    let (cd_offset, cd_size) = find_central_directory(src)?;
    let cd = src.read_at(cd_offset, cd_size)?;
    let mut picked: Vec<CdEntry> = parse_central_directory(&cd)
        .into_iter()
        .filter(|e| want(&e.name))
        .collect();
    picked.sort_by_key(|e| e.local_offset);
    let mut kept = Vec::new();
    for entry in picked {
        let lh = src.read_at(entry.local_offset, 30)?;
        if lh[0..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(format!("{} has a bad local header", entry.name));
        }
        let skip = 30 + u16_at(&lh, 26) as u64 + u16_at(&lh, 28) as u64;
        let data = src.read_at(entry.local_offset + skip, entry.comp_size)?;
        let plain = inflate(&entry, &data)?;
        let mut crc = flate2::Crc::new();
        crc.update(&plain);
        if crc.sum() != entry.crc || plain.len() as u64 != entry.size {
            return Err(format!(
                "{} failed its CRC-32 / size check; refusing to use it",
                entry.name
            ));
        }
        kept.push(KeptEntry { entry, data, plain });
    }
    let signing_block = read_signing_block(src, cd_offset)?;
    Ok((kept, signing_block))
}

/// Write the slim package: local records for the kept entries (no
/// data descriptors, no extra fields), the original signing block,
/// a rebuilt central directory and end record.
pub(crate) fn build_zip(kept: &[KeptEntry], signing_block: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for k in kept {
        let e = &k.entry;
        let offset = out.len() as u32;
        let flags = e.flags & !0x0008;
        let name = e.name.as_bytes();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&e.version_needed.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&e.time.to_le_bytes());
        out.extend_from_slice(&e.date.to_le_bytes());
        out.extend_from_slice(&e.crc.to_le_bytes());
        out.extend_from_slice(&(k.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.size as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&k.data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&e.version_made.to_le_bytes());
        central.extend_from_slice(&e.version_needed.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&e.method.to_le_bytes());
        central.extend_from_slice(&e.time.to_le_bytes());
        central.extend_from_slice(&e.date.to_le_bytes());
        central.extend_from_slice(&e.crc.to_le_bytes());
        central.extend_from_slice(&(k.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(e.size as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    out.extend_from_slice(signing_block);
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(kept.len() as u16).to_le_bytes());
    out.extend_from_slice(&(kept.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemRanges(Vec<u8>);
    impl RangeSource for MemRanges {
        fn len(&self) -> Result<u64, String> {
            Ok(self.0.len() as u64)
        }
        fn read_at(&mut self, start: u64, len: u64) -> Result<Vec<u8>, String> {
            Ok(self.0[start as usize..(start + len) as usize].to_vec())
        }
    }

    fn entry(name: &str, plain: &[u8], method: u16) -> KeptEntry {
        use std::io::Write;
        let data = if method == 8 {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(plain).unwrap();
            enc.finish().unwrap()
        } else {
            plain.to_vec()
        };
        let mut crc = flate2::Crc::new();
        crc.update(plain);
        KeptEntry {
            entry: CdEntry {
                name: name.to_string(),
                version_made: 20,
                version_needed: 20,
                flags: 0x0008,
                method,
                time: 0,
                date: 0,
                crc: crc.sum(),
                comp_size: data.len() as u64,
                size: plain.len() as u64,
                local_offset: 0,
            },
            data,
            plain: plain.to_vec(),
        }
    }

    fn fake_signing_block() -> Vec<u8> {
        let payload = b"PAYLOAD-BYTES-0123456789".to_vec();
        let size = (payload.len() + 8 + 16) as u64;
        let mut b = Vec::new();
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(&payload);
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(b"APK Sig Block 42");
        b
    }

    #[test]
    fn slim_round_trip_keeps_only_wanted_entries() {
        // A "full" package with three entries and a signing block.
        let full = build_zip(
            &[
                entry("classes.dex", &[7u8; 5000], 8),
                entry("lib/arm64-v8a/libsgmainso-1.so", b"SO-BYTES-SO-BYTES", 8),
                entry("META-INF/CERT.RSA", b"CERT", 0),
            ],
            &fake_signing_block(),
        );
        let mut src = MemRanges(full);
        let want = |n: &str| n != "classes.dex";
        let (kept, sig) = fetch_entries(&mut src, &want).unwrap();
        assert_eq!(sig, fake_signing_block());
        let names: Vec<_> = kept.iter().map(|k| k.entry.name.clone()).collect();
        assert_eq!(names, ["lib/arm64-v8a/libsgmainso-1.so", "META-INF/CERT.RSA"]);
        assert_eq!(kept[0].plain, b"SO-BYTES-SO-BYTES");

        // The slim package parses again and carries the same block.
        let slim = build_zip(&kept, &sig);
        let mut src2 = MemRanges(slim);
        let (kept2, sig2) = fetch_entries(&mut src2, &|_| true).unwrap();
        assert_eq!(sig2, sig);
        assert_eq!(kept2.len(), 2);
        assert_eq!(kept2[1].plain, b"CERT");
    }

    #[test]
    fn edge_lookup_answers_parse() {
        let ips = parse_httpdns("116.211.162.139;61.170.65.76,120
");
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[1].to_string(), "61.170.65.76");
        assert!(parse_httpdns("").is_empty());
        let doh = r#"{"Status":0,"Answer":[
            {"name":"a.","type":5,"data":"b."},
            {"name":"b.","type":1,"data":"61.170.65.76"},
            {"name":"b.","type":1,"data":"116.211.162.139"}]}"#;
        let ips = parse_doh_json(doh);
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0].to_string(), "61.170.65.76");
        assert!(parse_doh_json("not json").is_empty());
        assert_eq!(
            url_host("https://imtt.dd.qq.com/16891/apk/X.apk").as_deref(),
            Some("imtt.dd.qq.com")
        );
    }

    #[test]
    fn corrupted_entry_is_refused() {
        let mut e = entry("META-INF/CERT.RSA", b"CERT", 0);
        e.entry.crc ^= 1;
        let full = build_zip(&[e], &[]);
        let mut src = MemRanges(full);
        assert!(fetch_entries(&mut src, &|_| true).is_err());
    }
}
