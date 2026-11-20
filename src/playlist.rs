use crate::model::Station;
use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, path::PathBuf};
use tokio::io::AsyncReadExt;
use url::Url;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 500;

pub fn stream_url(value: &str) -> Result<Url> {
    let mut url = Url::parse(value.trim()).context("Enter a complete HTTP or HTTPS stream URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Only HTTP and HTTPS streams are supported");
    }
    url.set_fragment(None);
    Ok(url)
}
fn is_pls(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".pls")
}

pub async fn load(client: &reqwest::Client, source: &str, name: &str) -> Result<Vec<Station>> {
    let source = source.trim();
    if let Ok(url) = stream_url(source) {
        if !is_pls(&url) {
            return Ok(vec![Station::custom(url, name, None)]);
        }
        let mut response = client.get(url).send().await?.error_for_status()?;
        let base = response.url().clone();
        if response
            .content_length()
            .is_some_and(|s| s > MAX_BYTES as u64)
        {
            bail!("Playlist exceeds 1 MiB");
        }
        let mut bytes = vec![];
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_BYTES {
                bail!("Playlist exceeds 1 MiB");
            }
            bytes.extend_from_slice(&chunk);
        }
        return parse(&String::from_utf8_lossy(&bytes), Some(&base), source, name);
    }
    if source.contains("://") {
        bail!("Only HTTP/HTTPS URLs or local .pls files are supported");
    }
    let path = if let Some(rest) = source.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?).join(rest)
    } else {
        PathBuf::from(source)
    };
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pls"))
    {
        bail!("Enter an HTTP/HTTPS stream URL or a .pls file path");
    }
    // Reject special files before opening (opening a FIFO could otherwise block).
    let metadata = tokio::fs::metadata(&path)
        .await
        .with_context(|| format!("Cannot read {}", path.display()))?;
    if !metadata.is_file() {
        bail!("Playlist must be a regular file");
    }
    let file = tokio::fs::File::open(&path)
        .await
        .with_context(|| format!("Cannot open {}", path.display()))?;
    let mut bytes = vec![];
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_BYTES {
        bail!("Playlist exceeds 1 MiB");
    }
    parse(&String::from_utf8_lossy(&bytes), None, source, name)
}

pub fn parse(text: &str, base: Option<&Url>, source: &str, name: &str) -> Result<Vec<Station>> {
    if text.len() > MAX_BYTES {
        bail!("Playlist exceeds 1 MiB");
    }
    let mut files: BTreeMap<u32, String> = BTreeMap::new();
    let mut titles = BTreeMap::new();
    let mut in_playlist = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') {
            in_playlist = line.eq_ignore_ascii_case("[playlist]");
            continue;
        }
        if !in_playlist {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            if let Some(index) = key
                .strip_prefix("file")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| *n > 0)
            {
                files.insert(index, value.trim().to_owned());
            } else if let Some(index) = key
                .strip_prefix("title")
                .and_then(|s| s.parse::<u32>().ok())
            {
                titles.insert(index, value.trim().to_owned());
            }
        }
        if files.len() > MAX_ENTRIES || titles.len() > MAX_ENTRIES {
            bail!("Playlist exceeds {MAX_ENTRIES} entries");
        }
    }
    let mut entries = vec![];
    for (index, value) in files {
        let url = if let Some(base) = base {
            base.join(&value)
                .ok()
                .and_then(|url| stream_url(url.as_str()).ok())
        } else {
            stream_url(&value).ok()
        };
        if let Some(url) = url.filter(|u| !is_pls(u)) {
            if entries.iter().any(|s: &Station| s.url == url.as_str()) {
                continue;
            }
            entries.push(Station::custom(
                url,
                titles.get(&index).map(String::as_str).unwrap_or(""),
                Some(source.into()),
            ));
        }
    }
    if entries.is_empty() {
        bail!(
            "No supported streams found. PLS needs [playlist] and FileN=HTTP(S) entries; nested .pls/local media are not supported"
        );
    }
    if entries.len() == 1 && !name.trim().is_empty() {
        entries[0].name = crate::model::clean(name.trim());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bom_titles_and_noncontiguous_entries() {
        let entries = parse("\u{feff}[playlist]\r\nTitle3=Jazz\r\nFile3=https://example.com/jazz\r\nFile1=https://example.com/live\r\nLength3=-1", None, "radio.pls", "").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].name, "Jazz");
        assert!(entries[0].url.ends_with("/live"));
    }
    #[test]
    fn relative_remote_only_and_nested_rejection() {
        let text = "[playlist]\nFile1=../live\nFile2=file:///etc/passwd\nFile3=https://example.com/more.pls";
        assert!(parse(text, None, "local.pls", "").is_err());
        let entries = parse(
            text,
            Some(&Url::parse("https://example.com/radio/list.pls").unwrap()),
            "remote",
            "My radio",
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://example.com/live");
        assert_eq!(entries[0].name, "My radio");
    }
    #[test]
    fn rejects_bad_and_oversized_playlists() {
        for text in [
            "",
            "[playlist]\nFile1=javascript:alert(1)",
            "File1=https://example.com/",
        ] {
            assert!(parse(text, None, "test", "").is_err());
        }
        assert!(parse(&"x".repeat(MAX_BYTES + 1), None, "test", "").is_err());
    }
    #[tokio::test]
    async fn direct_stream_needs_no_network() {
        let entries = load(
            &reqwest::Client::new(),
            "https://not-a-real-host.invalid/live",
            "Offline",
        )
        .await
        .unwrap();
        assert_eq!(entries[0].name, "Offline");
    }
    #[tokio::test]
    async fn local_import_is_self_contained() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("radio.pls");
        std::fs::write(&path, "[playlist]\nFile1=https://example.com/live").unwrap();
        let entries = load(&reqwest::Client::new(), path.to_str().unwrap(), "")
            .await
            .unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(entries[0].url, "https://example.com/live");
    }
    #[tokio::test]
    async fn remote_playlist_resolves_against_final_redirect_url() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for i in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                crate::tests::request_headers(&mut socket).await;
                let response = if i == 0 {
                    "HTTP/1.1 302 Found\r\nLocation: /radio/stations.pls\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
                } else {
                    let body = "[playlist]\nFile2=../live\nTitle2=Redirect radio";
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let entries = load(&reqwest::Client::new(), &format!("{host}/redirect.pls"), "")
            .await
            .unwrap();
        assert_eq!(entries[0].url, format!("{host}/live"));
        assert_eq!(entries[0].name, "Redirect radio");
        server.await.unwrap();
    }
}
