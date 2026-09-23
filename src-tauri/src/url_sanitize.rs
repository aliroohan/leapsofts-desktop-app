use url::Url;

pub struct SanitizedUrl {
    pub url: String,
    pub domain: String,
}

pub fn sanitize_url(raw: Option<&str>) -> Option<SanitizedUrl> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = Url::parse(raw).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_string();
    let domain = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if domain.is_empty() {
        return None;
    }
    Some(SanitizedUrl {
        url: domain.clone(),
        domain,
    })
}
