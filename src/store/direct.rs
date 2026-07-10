//! Direct fetcher: DisplayCatalog + FE3 SOAP, no local dependencies.
//!
//! Flow:
//! 1. DisplayCatalog JSON lookup -> WuCategoryId
//! 2. FE3 GetCookie (anonymous SOAP) -> EncryptedData session token
//! 3. FE3 SyncUpdates (SOAP) -> list of UpdateIdentity + PackageMoniker
//! 4. FE3 GetExtendedUpdateInfo2 on /secured (SOAP) -> signed CDN download URL
//!
//! SOAP envelope templates were adapted from StoreDev/StoreLib (MIT).

use super::{DownloadResult, ResolvedProduct};
use anyhow::{anyhow, bail, Context, Result};
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;
use reqwest::blocking::Client;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

const DISPLAYCATALOG_BASE: &str = "https://displaycatalog.mp.microsoft.com/v7.0/products/";
const FE3_URL: &str = "https://fe3.delivery.mp.microsoft.com/ClientWebService/client.asmx";
const FE3_SECURED_URL: &str =
    "https://fe3.delivery.mp.microsoft.com/ClientWebService/client.asmx/secured";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

const GET_COOKIE_TEMPLATE: &str = include_str!("templates/GetCookie.xml");
const SYNC_UPDATES_TEMPLATE: &str = include_str!("templates/WUIDRequest.xml");
const FILE_URL_TEMPLATE: &str = include_str!("templates/FE3FileUrl.xml");

// Anonymous MSA device ticket replayed from a real Store session. The same
// value used by StoreDev/StoreLib — stable for years. Lets anonymous clients
// call GetExtendedUpdateInfo2 to resolve download URLs for free Store apps
// without needing a signed-in Microsoft account.
const MSA_TOKEN: &str = "<Device>dAA9AEUAdwBBAHcAQQBzAE4AMwBCAEEAQQBVADEAYgB5AHMAZQBtAGIAZQBEAFYAQwArADMAZgBtADcAbwBXAHkASAA3AGIAbgBnAEcAWQBtAEEAQQBMAGoAbQBqAFYAVQB2AFEAYwA0AEsAVwBFAC8AYwBDAEwANQBYAGUANABnAHYAWABkAGkAegBHAGwAZABjADEAZAAvAFcAeQAvAHgASgBQAG4AVwBRAGUAYwBtAHYAbwBjAGkAZwA5AGoAZABwAE4AawBIAG0AYQBzAHAAVABKAEwARAArAFAAYwBBAFgAbQAvAFQAcAA3AEgAagBzAEYANAA0AEgAdABsAC8AMQBtAHUAcgAwAFMAdQBtAG8AMABZAGEAdgBqAFIANwArADQAcABoAC8AcwA4ADEANgBFAFkANQBNAFIAbQBnAFIAQwA2ADMAQwBSAEoAQQBVAHYAZgBzADQAaQB2AHgAYwB5AEwAbAA2AHoAOABlAHgAMABrAFgAOQBPAHcAYQB0ADEAdQBwAFMAOAAxAEgANgA4AEEASABzAEoAegBnAFQAQQBMAG8AbgBBADIAWQBBAEEAQQBpAGcANQBJADMAUQAvAFYASABLAHcANABBAEIAcQA5AFMAcQBhADEAQgA4AGsAVQAxAGEAbwBLAEEAdQA0AHYAbABWAG4AdwBWADMAUQB6AHMATgBtAEQAaQBqAGgANQBkAEcAcgBpADgAQQBlAEUARQBWAEcAbQBXAGgASQBCAE0AUAAyAEQAVwA0ADMAZABWAGkARABUAHoAVQB0AHQARQBMAEgAaABSAGYAcgBhAGIAWgBsAHQAQQBUAEUATABmAHMARQBGAFUAYQBRAFMASgB4ADUAeQBRADgAagBaAEUAZQAyAHgANABCADMAMQB2AEIAMgBqAC8AUgBLAGEAWQAvAHEAeQB0AHoANwBUAHYAdAB3AHQAagBzADYAUQBYAEIAZQA4AHMAZwBJAG8AOQBiADUAQQBCADcAOAAxAHMANgAvAGQAUwBFAHgATgBEAEQAYQBRAHoAQQBYAFAAWABCAFkAdQBYAFEARQBzAE8AegA4AHQAcgBpAGUATQBiAEIAZQBUAFkAOQBiAG8AQgBOAE8AaQBVADcATgBSAEYAOQAzAG8AVgArAFYAQQBiAGgAcAAwAHAAUgBQAFMAZQBmAEcARwBPAHEAdwBTAGcANwA3AHMAaAA5AEoASABNAHAARABNAFMAbgBrAHEAcgAyAGYARgBpAEMAUABrAHcAVgBvAHgANgBuAG4AeABGAEQAbwBXAC8AYQAxAHQAYQBaAHcAegB5AGwATABMADEAMgB3AHUAYgBtADUAdQBtAHAAcQB5AFcAYwBLAFIAagB5AGgAMgBKAFQARgBKAFcANQBnAFgARQBJADUAcAA4ADAARwB1ADIAbgB4AEwAUgBOAHcAaQB3AHIANwBXAE0AUgBBAFYASwBGAFcATQBlAFIAegBsADkAVQBxAGcALwBwAFgALwB2AGUATAB3AFMAawAyAFMAUwBIAGYAYQBLADYAagBhAG8AWQB1AG4AUgBHAHIAOABtAGIARQBvAEgAbABGADYASgBDAGEAYQBUAEIAWABCAGMAdgB1AGUAQwBKAG8AOQA4AGgAUgBBAHIARwB3ADQAKwBQAEgAZQBUAGIATgBTAEUAWABYAHoAdgBaADYAdQBXADUARQBBAGYAZABaAG0AUwA4ADgAVgBKAGMAWgBhAEYASwA3AHgAeABnADAAdwBvAG4ANwBoADAAeABDADYAWgBCADAAYwBZAGoATAByAC8ARwBlAE8AegA5AEcANABRAFUASAA5AEUAawB5ADAAZAB5AEYALwByAGUAVQAxAEkAeQBpAGEAcABwAGgATwBQADgAUwAyAHQANABCAHIAUABaAFgAVAB2AEMAMABQADcAegBPACsAZgBHAGsAeABWAG0AKwBVAGYAWgBiAFEANQA1AHMAdwBFAD0AJgBwAD0A</Device>";

#[derive(Debug, Clone)]
struct PackageCandidate {
    moniker: String,
    update_id: String,
    revision_id: String,
    architecture: String,
}

struct CatalogProduct {
    category_id: String,
    title: String,
}

pub(super) fn download_latest(
    product_id: &str,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<DownloadResult> {
    let (url, moniker, version) = resolve_url(product_id)?;
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(format!("{moniker}.msix"));
    download_url(&url, &dest, progress)?;
    Ok(DownloadResult {
        msix_path: dest,
        moniker,
        version,
    })
}

/// Resolve just the latest version string for `product_id`, skipping the
/// file-URL + download steps. Cheaper than `download_latest` — used by the
/// update checker.
pub(super) fn resolve_latest_version(product_id: &str) -> Result<String> {
    Ok(resolve_latest_product(product_id)?.version)
}

pub(super) fn resolve_latest_product(product_id: &str) -> Result<ResolvedProduct> {
    let client = metadata_http_client()?;
    let catalog =
        fetch_catalog_product(&client, product_id).context("DisplayCatalog lookup failed")?;
    let cookie = get_cookie(&client).context("FE3 GetCookie failed")?;
    let sync_xml =
        sync_updates(&client, &cookie, &catalog.category_id).context("FE3 SyncUpdates failed")?;
    let candidates =
        parse_package_candidates(&sync_xml).context("Parsing SyncUpdates response failed")?;
    let best = pick_best_candidate(&candidates)
        .ok_or_else(|| anyhow!("no x64 Codex candidate in SyncUpdates response"))?;
    let version = moniker_version(&best.moniker)
        .ok_or_else(|| anyhow!("couldn't parse version from moniker: {}", best.moniker))?
        .to_string();
    Ok(ResolvedProduct {
        title: catalog.title,
        version,
    })
}

/// Debug helper exposed for `--dump-sync` CLI flag.
pub(super) fn debug_dump_sync_xml(product_id: &str) -> Result<String> {
    let client = metadata_http_client()?;
    let category_id = fetch_catalog_product(&client, product_id)?.category_id;
    let cookie = get_cookie(&client)?;
    sync_updates(&client, &cookie, &category_id)
}

/// Runs DisplayCatalog + full FE3 resolve chain. Returns (signed_cdn_url, moniker, msix_version).
fn resolve_url(product_id: &str) -> Result<(String, String, String)> {
    let client = metadata_http_client()?;
    let category_id = fetch_catalog_product(&client, product_id)
        .context("DisplayCatalog lookup failed")?
        .category_id;
    let cookie = get_cookie(&client).context("FE3 GetCookie failed")?;
    let sync_xml =
        sync_updates(&client, &cookie, &category_id).context("FE3 SyncUpdates failed")?;
    let candidates =
        parse_package_candidates(&sync_xml).context("Parsing SyncUpdates response failed")?;
    let best = pick_best_candidate(&candidates)
        .ok_or_else(|| anyhow!("no x64 Codex candidate in SyncUpdates response"))?;
    let version = moniker_version(&best.moniker)
        .ok_or_else(|| anyhow!("couldn't parse version from moniker: {}", best.moniker))?
        .to_string();
    let urls = get_file_urls(&client, &best.update_id, &best.revision_id)
        .context("FE3 GetExtendedUpdateInfo2 failed")?;
    let url = pick_msix_url(&urls)
        .ok_or_else(|| anyhow!("no usable MSIX URL in GetExtendedUpdateInfo2 response"))?;
    Ok((url, best.moniker.clone(), version))
}

fn download_url(
    url: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<()> {
    let client = download_http_client()?;
    let mut resp = client.get(url).send()?.error_for_status()?;
    let total = resp.content_length();
    let mut file = std::fs::File::create(dest)?;
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        written += n as u64;
        progress(written, total)?;
    }
    file.flush()?;
    Ok(())
}

// -- internals --------------------------------------------------------------

fn metadata_http_client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(15))
        .user_agent("Windows-Update-Agent/10.0.10011.16384 Client-Protocol/1.40")
        .build()?)
}

fn download_http_client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent("Windows-Update-Agent/10.0.10011.16384 Client-Protocol/1.40")
        .build()?)
}

fn fetch_catalog_product(client: &Client, product_id: &str) -> Result<CatalogProduct> {
    let url = format!(
        "{}{}?market=US&languages=en-US",
        DISPLAYCATALOG_BASE, product_id
    );
    let body: Value = client.get(&url).send()?.error_for_status()?.json()?;
    let title = body
        .pointer("/Product/LocalizedProperties/0/ProductTitle")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Codex")
        .to_string();
    let skus = body
        .pointer("/Product/DisplaySkuAvailabilities")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("missing DisplaySkuAvailabilities"))?;
    for sku in skus {
        let Some(fd) = sku.pointer("/Sku/Properties/FulfillmentData") else {
            continue;
        };
        // Different endpoints return FulfillmentData either as an already-parsed
        // object or as a JSON-encoded string. Handle both.
        let inner: std::borrow::Cow<Value> = match fd {
            Value::String(s) => std::borrow::Cow::Owned(
                serde_json::from_str(s).context("FulfillmentData string is not valid JSON")?,
            ),
            v => std::borrow::Cow::Borrowed(v),
        };
        if let Some(id) = inner.get("WuCategoryId").and_then(|v| v.as_str()) {
            return Ok(CatalogProduct {
                category_id: id.to_string(),
                title,
            });
        }
    }
    bail!("no WuCategoryId found in any SKU")
}

fn get_cookie(client: &Client) -> Result<String> {
    let resp = client
        .post(FE3_URL)
        .header("Content-Type", "application/soap+xml; charset=utf-8")
        .body(GET_COOKIE_TEMPLATE.to_string())
        .send()?
        .error_for_status()?;
    let xml = resp.text()?;
    extract_element_text(&xml, "EncryptedData")
        .ok_or_else(|| anyhow!("no EncryptedData element in GetCookie response"))
}

fn sync_updates(client: &Client, cookie: &str, category_id: &str) -> Result<String> {
    let body = SYNC_UPDATES_TEMPLATE
        .replacen("{0}", cookie, 1)
        .replacen("{1}", category_id, 1)
        .replacen("{2}", MSA_TOKEN, 1);
    let resp = client
        .post(FE3_URL)
        .header("Content-Type", "application/soap+xml; charset=utf-8")
        .body(body)
        .send()?
        .error_for_status()?;
    let xml = resp.text()?;
    // Inner XML arrives HTML-escaped inside SOAP text nodes.
    Ok(html_decode(&xml))
}

fn get_file_urls(client: &Client, update_id: &str, revision_id: &str) -> Result<Vec<String>> {
    let body = FILE_URL_TEMPLATE
        .replacen("{0}", update_id, 1)
        .replacen("{1}", revision_id, 1)
        .replacen("{2}", MSA_TOKEN, 1);
    let resp = client
        .post(FE3_SECURED_URL)
        .header("Content-Type", "application/soap+xml; charset=utf-8")
        .body(body)
        .send()?
        .error_for_status()?;
    let xml = resp.text()?;
    Ok(extract_file_urls(&xml))
}

fn pick_best_candidate(cands: &[PackageCandidate]) -> Option<&PackageCandidate> {
    // FE3 SyncUpdates can return multiple applicable x64 packages in
    // arbitrary order. Pick the highest dotted-numeric version rather
    // than the first match so we never downgrade an update check.
    cands
        .iter()
        .filter(|c| {
            c.architecture.eq_ignore_ascii_case("x64") && c.moniker.starts_with("OpenAI.Codex_")
        })
        .max_by(|a, b| {
            let va = parse_version(moniker_version(&a.moniker).unwrap_or(""));
            let vb = parse_version(moniker_version(&b.moniker).unwrap_or(""));
            va.cmp(&vb)
        })
}

fn parse_version(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn pick_msix_url(urls: &[String]) -> Option<String> {
    // The response returns multiple URLs per update — blockmap (short, ~99
    // chars per StoreLib's observation) plus the actual signed CDN URL for
    // the package (much longer). Take the longest http(s) URL.
    urls.iter()
        .filter(|u| u.starts_with("http"))
        .filter(|u| u.len() != 99)
        .cloned()
        .max_by_key(|u| u.len())
}

fn moniker_version(moniker: &str) -> Option<&str> {
    moniker.split('_').nth(1)
}

fn moniker_arch(moniker: &str) -> Option<String> {
    moniker.split('_').nth(2).map(|s| s.to_string())
}

// -- XML plumbing -----------------------------------------------------------

fn html_decode(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().rposition(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

fn extract_element_text(xml: &str, tag: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut depth_in = 0u32;
    let mut collected = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == tag.as_bytes() => {
                depth_in = depth_in.saturating_add(1);
            }
            Ok(Event::End(e))
                if local_name(e.name().as_ref()) == tag.as_bytes() && depth_in > 0 =>
            {
                return Some(collected);
            }
            Ok(Event::Text(t)) if depth_in > 0 => {
                if let Ok(decoded) = t.decode() {
                    if let Ok(s) = unescape(&decoded) {
                        collected.push_str(&s);
                    }
                }
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Stream-parse a SyncUpdates SOAP response. The leaf downloadable packages
/// live inside `<UpdateInfo><Xml>...</Xml></UpdateInfo>` elements. Each such
/// UpdateInfo we care about has:
/// - exactly one top-level `<UpdateIdentity UpdateID=".." RevisionNumber=".."/>`
///   (subsequent UpdateIdentity elements nested in Relationships are
///   prerequisite references, not the package's own identity — we skip them)
/// - a `<SecuredFragment>FileUrl</SecuredFragment>` marker under Properties
/// - an `<AppxMetadata PackageMoniker="..."/>` deep under ApplicabilityRules
fn parse_package_candidates(xml: &str) -> Result<Vec<PackageCandidate>> {
    let mut out: Vec<PackageCandidate> = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<PendingUpdate> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match local_name(e.name().as_ref()) {
                    b"UpdateInfo" => {
                        // Only actually push on Start; Empty (<UpdateInfo/>) is nonsensical.
                        // We don't need to inspect attrs — just track scope via the stack.
                        stack.push(PendingUpdate::default());
                    }
                    b"UpdateIdentity" => {
                        if let Some(top) = stack.last_mut() {
                            read_identity_attrs(&e, top);
                        }
                    }
                    b"SecuredFragment" => {
                        if let Some(top) = stack.last_mut() {
                            top.has_secured_fragment = true;
                        }
                    }
                    b"AppxMetadata" => {
                        if let Some(top) = stack.last_mut() {
                            read_appx_attrs(&e, top);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == b"UpdateInfo" => {
                if let Some(p) = stack.pop() {
                    if p.has_secured_fragment {
                        if let (Some(u), Some(r), Some(m)) = (p.update_id, p.revision_id, p.moniker)
                        {
                            let arch = moniker_arch(&m).unwrap_or_default();
                            out.push(PackageCandidate {
                                moniker: m,
                                update_id: u,
                                revision_id: r,
                                architecture: arch,
                            });
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("XML parse error: {}", e),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

#[derive(Default)]
struct PendingUpdate {
    update_id: Option<String>,
    revision_id: Option<String>,
    moniker: Option<String>,
    has_secured_fragment: bool,
}

fn read_identity_attrs(e: &quick_xml::events::BytesStart, p: &mut PendingUpdate) {
    // Only capture the FIRST UpdateIdentity per UpdateInfo. Later ones in
    // Relationships/BundledUpdates are prerequisite references.
    if p.update_id.is_some() {
        return;
    }
    for attr in e.attributes().flatten() {
        let key = local_name(attr.key.as_ref()).to_vec();
        let val = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .ok()
            .map(|c| c.into_owned());
        match (key.as_slice(), val) {
            (b"UpdateID", Some(v)) => p.update_id = Some(v),
            (b"RevisionNumber", Some(v)) => p.revision_id = Some(v),
            _ => {}
        }
    }
}

fn read_appx_attrs(e: &quick_xml::events::BytesStart, p: &mut PendingUpdate) {
    for attr in e.attributes().flatten() {
        let key = local_name(attr.key.as_ref()).to_vec();
        if key == b"PackageMoniker" {
            if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                p.moniker = Some(v.into_owned());
            }
        }
    }
}

fn extract_file_urls(xml: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_file_location = false;
    let mut in_url = false;
    let mut current_url = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"FileLocation" => in_file_location = true,
                b"Url" if in_file_location => {
                    in_url = true;
                    current_url.clear();
                }
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"FileLocation" => in_file_location = false,
                b"Url" => {
                    in_url = false;
                    if !current_url.is_empty() {
                        urls.push(current_url.clone());
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_url => {
                if let Ok(decoded) = t.decode() {
                    if let Ok(s) = unescape(&decoded) {
                        current_url.push_str(&s);
                    }
                }
            }
            Ok(Event::GeneralRef(r)) if in_url => {
                append_xml_ref(&mut current_url, r.as_ref());
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    urls
}

fn append_xml_ref(out: &mut String, raw: &[u8]) {
    match raw {
        b"amp" => out.push('&'),
        b"lt" => out.push('<'),
        b"gt" => out.push('>'),
        b"quot" => out.push('"'),
        b"apos" => out.push('\''),
        _ => {
            out.push('&');
            out.push_str(&String::from_utf8_lossy(raw));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_file_urls_keeps_signed_query_params_together() {
        let xml = r#"
            <ExtendedUpdateInfo>
              <FileLocation>
                <Url>http://tlu.dl.delivery.mp.microsoft.com/filestreamingservice/files/pkg?P1=1&amp;P2=404&amp;P3=2&amp;P4=sig%3d%3d</Url>
              </FileLocation>
            </ExtendedUpdateInfo>
        "#;

        assert_eq!(
            extract_file_urls(xml),
            vec![
                "http://tlu.dl.delivery.mp.microsoft.com/filestreamingservice/files/pkg?P1=1&P2=404&P3=2&P4=sig%3d%3d"
                    .to_string()
            ]
        );
    }

    #[test]
    fn direct_download_has_a_bounded_timeout_that_still_allows_slow_large_downloads() {
        assert_eq!(DOWNLOAD_TIMEOUT, Duration::from_secs(30 * 60));
        assert!(
            include_str!("direct.rs")
                .matches(".timeout(DOWNLOAD_TIMEOUT)")
                .count()
                >= 2
        );
    }
}
