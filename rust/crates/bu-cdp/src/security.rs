//! URL access-policy enforcement — a faithful port of Python's SecurityWatchdog
//! (`browser_use/browser/watchdogs/security_watchdog.py`): `_is_url_allowed`,
//! `_is_url_match`, and `_is_ip_address`.

use url::Url;

/// Domain/IP access policy applied to every navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UrlPolicy {
    /// Allowlist patterns; empty means "no allow restriction".
    pub allowed_domains: Vec<String>,
    /// Denylist patterns; consulted only when `allowed_domains` is empty.
    pub prohibited_domains: Vec<String>,
    /// Reject navigations to bare IP literals (SSRF hardening).
    pub block_ip_addresses: bool,
}

impl UrlPolicy {
    /// Loads a base policy from the environment:
    /// `BROWSER_USE_ALLOWED_DOMAINS` / `BROWSER_USE_PROHIBITED_DOMAINS`
    /// (comma-separated) and `BROWSER_USE_BLOCK_IP_ADDRESSES` (truthy).
    pub fn from_env() -> Self {
        Self {
            allowed_domains: csv_env("BROWSER_USE_ALLOWED_DOMAINS"),
            prohibited_domains: csv_env("BROWSER_USE_PROHIBITED_DOMAINS"),
            block_ip_addresses: truthy_env("BROWSER_USE_BLOCK_IP_ADDRESSES"),
        }
    }

    /// True when the policy imposes no restriction at all (fast path).
    pub fn is_unrestricted(&self) -> bool {
        self.allowed_domains.is_empty()
            && self.prohibited_domains.is_empty()
            && !self.block_ip_addresses
    }

    /// Mirrors `SecurityWatchdog._is_url_allowed`.
    pub fn is_url_allowed(&self, url: &str) -> bool {
        // Always allow internal browser targets.
        if matches!(
            url,
            "about:blank" | "chrome://new-tab-page/" | "chrome://new-tab-page" | "chrome://newtab/"
        ) {
            return true;
        }

        let parsed = match Url::parse(url) {
            Ok(parsed) => parsed,
            Err(_) => return false,
        };

        let scheme = parsed.scheme();
        // data: and blob: URLs have no host.
        if scheme == "data" || scheme == "blob" {
            return true;
        }

        let host = match parsed.host_str() {
            Some(host) if !host.is_empty() => host.to_ascii_lowercase(),
            _ => return false,
        };

        if self.block_ip_addresses && is_ip_address(&host) {
            return false;
        }

        if self.allowed_domains.is_empty() && self.prohibited_domains.is_empty() {
            return true;
        }

        if !self.allowed_domains.is_empty() {
            return self
                .allowed_domains
                .iter()
                .any(|pattern| is_url_match(url, &host, scheme, pattern));
        }

        if !self.prohibited_domains.is_empty() {
            return !self
                .prohibited_domains
                .iter()
                .any(|pattern| is_url_match(url, &host, scheme, pattern));
        }

        true
    }
}

/// Mirrors `SecurityWatchdog._is_url_match`.
fn is_url_match(url: &str, host: &str, scheme: &str, pattern: &str) -> bool {
    let full_url_pattern = format!("{scheme}://{host}");

    if pattern.contains('*') {
        if let Some(domain_part) = pattern.strip_prefix("*.") {
            // *.example.com matches subdomains AND the main domain, http(s) only.
            if (host == domain_part || host.ends_with(&format!(".{domain_part}")))
                && (scheme == "http" || scheme == "https")
            {
                return true;
            }
        } else if pattern.ends_with("/*") {
            if glob_match(pattern, url) {
                return true;
            }
        } else {
            let target = if pattern.contains("://") {
                full_url_pattern.as_str()
            } else {
                host
            };
            if glob_match(pattern, target) {
                return true;
            }
        }
        false
    } else if pattern.contains("://") {
        // Full-URL exact prefix.
        url.starts_with(pattern)
    } else {
        // Domain-only, case-insensitive.
        let pattern_lower = pattern.to_ascii_lowercase();
        if host == pattern_lower {
            return true;
        }
        is_root_domain(pattern) && host == format!("www.{pattern_lower}")
    }
}

/// A simple root domain (exactly one dot, no wildcard/scheme) — mirrors
/// `_is_root_domain`, which also allows the `www.` subdomain.
fn is_root_domain(pattern: &str) -> bool {
    if pattern.contains('*') || pattern.contains("://") {
        return false;
    }
    pattern.matches('.').count() == 1
}

/// fnmatch-style glob supporting `*` (any run, including `/`) and `?` (one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_ti = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// True iff `host` is an IPv4/IPv6 literal the browser would resolve, including
/// non-standard IPv4 encodings (decimal, hex, octal, short-form) that
/// `inet_aton` accepts. Mirrors `_is_ip_address`. Never panics.
///
/// Note: exotic Unicode-digit / IDNA-separator canonicalization (NFKC, U+3002)
/// from the Python port is omitted; `block_ip_addresses` is off by default and
/// the common SSRF vectors (dotted, decimal, hex/octal) are covered.
fn is_ip_address(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let decoded = percent_decode(bare);
    let candidate = decoded.trim();

    if candidate.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    inet_aton_accepts(candidate)
}

/// Accepts the liberal IPv4 forms `inet_aton` does: 1–4 dotted parts, each
/// decimal / 0x-hex / 0-octal, plus a single packed integer.
fn inet_aton_accepts(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return false;
    }
    if parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    // Each part must parse as an integer in some radix and fit inet_aton's field
    // widths (last/only part is wider). We only need a yes/no, so bound-check.
    let max_for = |index: usize, len: usize| -> u64 {
        if index + 1 == len {
            // trailing field absorbs the remaining bytes
            match len {
                1 => u32::MAX as u64,
                2 => 0x00ff_ffff,
                3 => 0x0000_ffff,
                _ => 0xff,
            }
        } else {
            0xff
        }
    };
    for (index, part) in parts.iter().enumerate() {
        let value = match parse_radix(part) {
            Some(value) => value,
            None => return false,
        };
        if value > max_for(index, parts.len()) {
            return false;
        }
    }
    true
}

fn parse_radix(part: &str) -> Option<u64> {
    let lower = part.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        if hex.is_empty() {
            return None;
        }
        u64::from_str_radix(hex, 16).ok()
    } else if lower.len() > 1 && lower.starts_with('0') {
        u64::from_str_radix(&lower, 8).ok()
    } else {
        lower.parse::<u64>().ok()
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn csv_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_owned())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn truthy_env(key: &str) -> bool {
    std::env::var(key)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allowed: &[&str]) -> UrlPolicy {
        UrlPolicy {
            allowed_domains: allowed.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn internal_targets_and_data_urls_always_allowed() {
        let p = policy(&["example.com"]);
        assert!(p.is_url_allowed("about:blank"));
        assert!(p.is_url_allowed("chrome://new-tab-page/"));
        assert!(p.is_url_allowed("data:text/html,<h1>x</h1>"));
    }

    #[test]
    fn empty_policy_allows_everything() {
        assert!(UrlPolicy::default().is_url_allowed("https://anything.example/"));
    }

    #[test]
    fn allowlist_blocks_off_domain_and_allows_www_variant() {
        let p = policy(&["example.com"]);
        assert!(p.is_url_allowed("https://example.com/path"));
        assert!(p.is_url_allowed("https://www.example.com/path"));
        assert!(!p.is_url_allowed("https://evil.com/"));
    }

    #[test]
    fn wildcard_subdomain_matches_sub_and_apex_http_only() {
        let p = policy(&["*.example.com"]);
        assert!(p.is_url_allowed("https://app.example.com/"));
        assert!(p.is_url_allowed("https://example.com/"));
        assert!(!p.is_url_allowed("https://example.com.evil.com/"));
    }

    #[test]
    fn full_url_prefix_pattern() {
        let p = policy(&["https://example.com/safe"]);
        assert!(p.is_url_allowed("https://example.com/safe/page"));
        assert!(!p.is_url_allowed("https://example.com/other"));
    }

    #[test]
    fn prohibited_list_blocks_only_matches() {
        let p = UrlPolicy {
            prohibited_domains: vec!["evil.com".into()],
            ..Default::default()
        };
        assert!(!p.is_url_allowed("https://evil.com/"));
        assert!(p.is_url_allowed("https://good.com/"));
    }

    #[test]
    fn block_ip_addresses_rejects_literals_and_ssrf() {
        let p = UrlPolicy {
            block_ip_addresses: true,
            ..Default::default()
        };
        assert!(!p.is_url_allowed("http://169.254.169.254/latest/meta-data/"));
        assert!(!p.is_url_allowed("http://127.0.0.1/"));
        assert!(!p.is_url_allowed("http://[::1]/"));
        // decimal-packed form of 127.0.0.1
        assert!(!p.is_url_allowed("http://2130706433/"));
        // hostnames are not IPs
        assert!(p.is_url_allowed("https://example.com/"));
    }

    #[test]
    fn invalid_url_is_rejected_under_a_policy() {
        let p = policy(&["example.com"]);
        assert!(!p.is_url_allowed("not a url"));
    }

    #[test]
    fn glob_matcher_basics() {
        assert!(glob_match("brave://*", "brave://settings"));
        assert!(glob_match(
            "http*://example.com/*",
            "https://example.com/a/b"
        ));
        assert!(!glob_match("http://example.com/*", "https://example.com/a"));
    }
}
