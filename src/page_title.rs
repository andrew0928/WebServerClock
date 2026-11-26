use std::time::Duration;

use encoding_rs::{Encoding, UTF_8};
use reqwest::{Client, Url, header};
use scraper::{Html, Selector};

const MAX_BYTES: usize = 64 * 1024;

pub async fn fetch(client: &Client, url: &Url) -> Option<String> {
    let mut response = client
        .get(url.clone())
        .header(header::ACCEPT, "text/html, application/xhtml+xml")
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok()?.parse::<mime::Mime>().ok());
    if let Some(kind) = &content_type
        && !matches!(kind.essence_str(), "text/html" | "application/xhtml+xml")
    {
        return None;
    }
    let encoding = content_type
        .as_ref()
        .and_then(|kind| kind.get_param(mime::CHARSET))
        .and_then(|name| Encoding::for_label(name.as_str().as_bytes()))
        .unwrap_or(UTF_8);
    let mut bytes = Vec::new();
    while bytes.len() < MAX_BYTES {
        let Some(chunk) = response.chunk().await.ok()? else {
            return title_from_html(&bytes, encoding);
        };
        let start = bytes.len().saturating_sub(7);
        bytes.extend_from_slice(&chunk[..chunk.len().min(MAX_BYTES - bytes.len())]);
        if bytes[start..]
            .windows(8)
            .any(|w| w.eq_ignore_ascii_case(b"</title>"))
            && let Some(title) = title_from_html(&bytes, encoding)
        {
            return Some(title);
        }
    }
    None
}

fn title_from_html(bytes: &[u8], encoding: &'static Encoding) -> Option<String> {
    let (text, _, _) = encoding.decode(bytes);
    let html = Html::parse_document(&text);
    let selector = Selector::parse("head > title").expect("valid selector");
    let text = html.select(&selector).next()?.text().collect::<String>();
    let title = text
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_page_title_as_plain_terminal_text() {
        let doc = "<!-- <title>假</title> --><script>let x='<title>假的</title>'</script><TiTlE>  臺北\n &amp; \x1b申請 </TiTlE>";
        assert_eq!(
            title_from_html(doc.as_bytes(), UTF_8).as_deref(),
            Some("臺北 & 申請")
        );
        assert_eq!(
            title_from_html(b"<svg><title>icon</title></svg>", UTF_8),
            None
        );
    }
}
