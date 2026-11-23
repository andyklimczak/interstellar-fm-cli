use crate::{
    model::{Station, StationId, clean},
    playlist::stream_url,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, OnceCell};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    Clicks,
    Name,
    ClickTrend,
    #[default]
    Votes,
}
impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clicks => "Clicks",
            Self::Name => "Name",
            Self::ClickTrend => "Click trend",
            Self::Votes => "Votes",
        }
    }
    pub fn field(self) -> &'static str {
        match self {
            Self::Clicks => "clickcount",
            Self::Name => "name",
            Self::ClickTrend => "clicktrend",
            Self::Votes => "votes",
        }
    }
    pub fn descending(self, reversed: bool) -> bool {
        (self != Self::Name) ^ reversed
    }
    pub fn next(self) -> Self {
        match self {
            Self::Clicks => Self::Name,
            Self::Name => Self::ClickTrend,
            Self::ClickTrend => Self::Votes,
            Self::Votes => Self::Clicks,
        }
    }
}

#[derive(Clone)]
pub struct RadioBrowser {
    client: reqwest::Client,
    hosts: Arc<Mutex<Vec<String>>>,
    discovery: Arc<OnceCell<()>>,
    discover: bool,
    rate_limit: Arc<Mutex<Option<tokio::time::Instant>>>,
}
impl RadioBrowser {
    pub fn new(client: reqwest::Client, base: Option<String>) -> Self {
        let discover = base.is_none();
        let hosts = if let Some(base) = base {
            vec![base]
        } else {
            let mut hosts = vec![
                "https://de1.api.radio-browser.info".into(),
                "https://de2.api.radio-browser.info".into(),
            ];
            let index = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as usize
                % hosts.len();
            hosts.rotate_left(index);
            hosts
        };
        Self {
            client,
            hosts: Arc::new(Mutex::new(hosts)),
            discovery: Arc::new(OnceCell::new()),
            discover,
            rate_limit: Arc::new(Mutex::new(None)),
        }
    }
    async fn discover_hosts(&self) {
        self.discovery
            .get_or_init(|| async {
                if !self.discover {
                    return;
                }
                #[derive(Deserialize)]
                struct Server {
                    name: String,
                }
                let result = tokio::time::timeout(
                    Duration::from_secs(4),
                    self.request::<Vec<Server>>("json/servers", &[]),
                )
                .await;
                if let Ok(Ok(servers)) = result {
                    let mut hosts = self.hosts.lock().await;
                    for server in servers.into_iter().take(30) {
                        if let Some(label) = server.name.strip_suffix(".api.radio-browser.info")
                            && !label.is_empty()
                            && label
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        {
                            let host = format!("https://{}", server.name);
                            if !hosts.contains(&host) {
                                hosts.push(host);
                            }
                        }
                    }
                }
            })
            .await;
    }
    async fn request<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        if let Some(until) = *self.rate_limit.lock().await
            && until > tokio::time::Instant::now()
        {
            bail!(
                "Radio Browser is rate-limiting requests; retry in {} seconds",
                until.duration_since(tokio::time::Instant::now()).as_secs() + 1
            );
        }
        let hosts = self.hosts.lock().await.clone();
        let mut last_error = "No API mirrors configured".to_string();
        tokio::time::timeout(Duration::from_secs(18), async {
            for host in hosts {
                let result: Result<T> = async {
                    let mut response = self
                        .client
                        .get(format!("{}/{path}", host.trim_end_matches('/')))
                        .query(params)
                        .send()
                        .await?;
                    let status = response.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let seconds = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(30)
                            .clamp(1, 3600);
                        *self.rate_limit.lock().await =
                            Some(tokio::time::Instant::now() + Duration::from_secs(seconds));
                        bail!(
                            "Radio Browser is rate-limiting requests; retry in {seconds} seconds"
                        );
                    }
                    response = response.error_for_status()?;
                    let mut bytes = vec![];
                    while let Some(chunk) = response.chunk().await? {
                        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                            bail!("API response is too large");
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(serde_json::from_slice(&bytes)?)
                }
                .await;
                match result {
                    Ok(value) => {
                        let mut preferred = self.hosts.lock().await;
                        if let Some(index) = preferred.iter().position(|s| *s == host) {
                            preferred.rotate_left(index);
                        }
                        return Ok(value);
                    }
                    Err(error) => {
                        last_error = error.to_string();
                        if last_error.contains("rate-limiting") {
                            break;
                        }
                        if let Some(status) = error
                            .downcast_ref::<reqwest::Error>()
                            .and_then(|e| e.status())
                            && status.is_client_error()
                        {
                            break;
                        }
                    }
                }
            }
            bail!("Radio Browser unavailable: {last_error}")
        })
        .await
        .context("Radio Browser request timed out; press r to retry")?
    }
    pub async fn search(&self, query: &str, sort: Sort, reversed: bool) -> Result<Vec<Station>> {
        self.discover_hosts().await;
        let query = query.trim();
        let searching = query.chars().count() >= 2;
        let mut params = vec![
            ("limit", if searching { "50" } else { "30" }.into()),
            ("hidebroken", "true".into()),
            ("order", sort.field().into()),
            ("reverse", sort.descending(reversed).to_string()),
        ];
        if searching {
            params.push(("name", query.into()));
        }
        // Browse and typed searches honor the same sort, including default Votes.
        let payload: Vec<RadioStation> = self.request("json/stations/search", &params).await?;
        let mut seen = HashSet::new();
        Ok(payload
            .into_iter()
            .filter(|s| s.lastcheckok.unwrap_or(1) == 1)
            .filter_map(RadioStation::normalize)
            .filter(|s| match &s.id {
                StationId::RadioBrowser(id) => seen.insert(id.clone()),
                _ => false,
            })
            .collect())
    }
    pub async fn refresh(&self, id: &str) -> Result<Station> {
        // UUID is validated before using it as a path component.
        let id = uuid::Uuid::parse_str(id)?;
        let payload: Vec<RadioStation> = self
            .request(&format!("json/stations/byuuid/{id}"), &[])
            .await?;
        payload
            .into_iter()
            .find_map(RadioStation::normalize)
            .context("Station no longer listed")
    }
    pub async fn click(&self, id: &str) {
        if let Ok(id) = uuid::Uuid::parse_str(id) {
            let _: Result<serde_json::Value> = self.request(&format!("json/url/{id}"), &[]).await;
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RadioStation {
    stationuuid: Option<String>,
    name: Option<String>,
    url: Option<String>,
    url_resolved: Option<String>,
    countrycode: Option<String>,
    codec: Option<String>,
    bitrate: Option<u32>,
    votes: Option<u64>,
    clickcount: Option<u64>,
    tags: Option<String>,
    language: Option<String>,
    homepage: Option<String>,
    lastcheckok: Option<i32>,
}
impl RadioStation {
    fn normalize(self) -> Option<Station> {
        let id = self.stationuuid.filter(|id| !id.is_empty())?;
        let url = self
            .url_resolved
            .and_then(|u| stream_url(&u).ok())
            .or_else(|| self.url.and_then(|u| stream_url(&u).ok()))?;
        let country_code = self
            .countrycode
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        let country_code =
            if country_code.len() == 2 && country_code.bytes().all(|b| b.is_ascii_alphabetic()) {
                country_code
            } else {
                String::new()
            };
        Some(Station {
            id: StationId::RadioBrowser(id),
            name: clean(&self.name.unwrap_or_else(|| "Unnamed station".into())),
            url: url.into(),
            country_code,
            legacy_country: String::new(),
            codec: self.codec.unwrap_or_default(),
            bitrate: self.bitrate.unwrap_or_default(),
            votes: self.votes,
            clicks: self.clickcount,
            tags: self.tags.unwrap_or_default(),
            language: self.language.unwrap_or_default(),
            homepage: self.homepage.unwrap_or_default(),
            playlist_source: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    #[test]
    fn sort_cycle_includes_all_options_and_wraps() {
        assert_eq!(Sort::default(), Sort::Votes);
        let options = [Sort::Clicks, Sort::Name, Sort::ClickTrend, Sort::Votes];
        for (i, sort) in options.iter().enumerate() {
            assert_eq!(sort.next(), options[(i + 1) % options.len()]);
        }
    }

    #[test]
    fn textual_sorts_are_ascending_and_numeric_sorts_descending() {
        assert!(!Sort::Name.descending(false));
        assert!(Sort::Name.descending(true));
        for sort in [Sort::Clicks, Sort::ClickTrend, Sort::Votes] {
            assert!(sort.descending(false));
            assert!(!sort.descending(true));
        }
    }

    #[tokio::test]
    async fn sort_options_send_the_correct_api_fields_and_direction() {
        for (sort, field) in [
            (Sort::Clicks, "clickcount"),
            (Sort::Name, "name"),
            (Sort::ClickTrend, "clicktrend"),
            (Sort::Votes, "votes"),
        ] {
            for reversed in [false, true] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let base = format!("http://{}", listener.local_addr().unwrap());
                let worker = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let request = crate::tests::request_headers(&mut socket).await;
                    assert!(request.contains(&format!("order={field}")));
                    assert!(request.contains(&format!("reverse={}", sort.descending(reversed))));
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]",
                        )
                        .await
                        .unwrap();
                });
                let client = RadioBrowser::new(reqwest::Client::new(), Some(base));
                assert!(
                    client
                        .search("jazz", sort, reversed)
                        .await
                        .unwrap()
                        .is_empty()
                );
                worker.await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn empty_and_short_queries_browse_by_votes_by_default() {
        for query in ["", "j"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let worker = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = crate::tests::request_headers(&mut socket).await;
                assert!(request.starts_with("GET /json/stations/search?"));
                assert!(request.contains("order=votes"));
                assert!(request.contains("reverse=true"));
                assert!(request.contains("limit=30"));
                assert!(!request.contains("name="));
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]",
                    )
                    .await
                    .unwrap();
            });
            let client = RadioBrowser::new(reqwest::Client::new(), Some(base));
            assert!(
                client
                    .search(query, Sort::default(), false)
                    .await
                    .unwrap()
                    .is_empty()
            );
            worker.await.unwrap();
        }
    }

    #[test]
    fn tolerates_nulls_and_falls_back_to_original_url() {
        let dto: RadioStation = serde_json::from_str(r#"{"stationuuid":"abc","name":"Radio","url":"http://example.com/live","url_resolved":"","country":null,"bitrate":null,"extra":42}"#).unwrap();
        let s = dto.normalize().unwrap();
        assert_eq!(s.url, "http://example.com/live");
        assert_eq!(s.bitrate, 0);
        assert_eq!((s.votes, s.clicks), (None, None));
    }

    #[test]
    fn uses_normalized_countrycode_and_never_the_deprecated_country_field() {
        for (code, expected) in [
            (serde_json::json!("US"), "US"),
            (serde_json::json!(" de "), "DE"),
            (serde_json::json!(null), ""),
            (serde_json::json!(""), ""),
            (serde_json::json!("USA"), ""),
            (serde_json::json!("1A"), ""),
            (serde_json::json!("é"), ""),
        ] {
            let mut value = serde_json::json!({
                "stationuuid": "abc", "url": "https://example.com/live",
                "countrycode": code, "country": "Do not use this deprecated name"
            });
            let station = serde_json::from_value::<RadioStation>(value.clone())
                .unwrap()
                .normalize()
                .unwrap();
            assert_eq!(station.country_code, expected);
            assert!(station.legacy_country.is_empty());
            value.as_object_mut().unwrap().remove("countrycode");
            let missing = serde_json::from_value::<RadioStation>(value)
                .unwrap()
                .normalize()
                .unwrap();
            assert_eq!(missing.country(), "—");
        }
    }

    #[test]
    fn normalizes_votes_and_clicks_without_confusing_missing_with_zero() {
        let station = |counts: &str| {
            serde_json::from_str::<RadioStation>(&format!(
                r#"{{"stationuuid":"abc","url":"https://example.com/live",{counts}}}"#
            ))
            .unwrap()
            .normalize()
            .unwrap()
        };
        let known = station(r#""votes":12345,"clickcount":67890"#);
        assert_eq!((known.votes, known.clicks), (Some(12345), Some(67890)));
        let zero = station(r#""votes":0,"clickcount":0"#);
        assert_eq!((zero.votes, zero.clicks), (Some(0), Some(0)));
        let unknown = station(r#""votes":null,"clickcount":null"#);
        assert_eq!((unknown.votes, unknown.clicks), (None, None));
    }
    #[tokio::test]
    async fn search_encodes_query_and_filters_broken_duplicates() {
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", server.local_addr().unwrap());
        let worker = tokio::spawn(async move {
            let (mut socket, _) = server.accept().await.unwrap();
            let request = crate::tests::request_headers(&mut socket).await;
            assert!(request.contains("name=jazz+%26+soul"));
            assert!(request.contains("reverse=true"));
            assert!(request.contains("hidebroken=true"));
            let body = r#"[{"stationuuid":"a","url":"https://example.com/live"},{"stationuuid":"a","url":"https://example.com/live"},{"stationuuid":"b","url":"https://example.com/broken","lastcheckok":0}]"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let client = RadioBrowser::new(reqwest::Client::new(), Some(base));
        assert_eq!(
            client
                .search("jazz & soul", Sort::Votes, false)
                .await
                .unwrap()
                .len(),
            1
        );
        worker.await.unwrap();
    }

    async fn mock(
        status: &str,
        headers: &str,
        body: &str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            crate::tests::request_headers(&mut socket).await;
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (host, task)
    }

    #[tokio::test]
    async fn transient_failure_uses_and_remembers_next_mirror() {
        let (bad, bad_task) = mock("503 Unavailable", "", "{}").await;
        let (good, good_task) = mock("200 OK", "", "[]").await;
        let client = RadioBrowser::new(reqwest::Client::new(), Some(bad.clone()));
        client.hosts.lock().await.push(good.clone());
        assert!(
            client
                .search("jazz", Sort::Name, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(client.hosts.lock().await[0], good);
        bad_task.await.unwrap();
        good_task.await.unwrap();
    }

    #[tokio::test]
    async fn rate_limit_stops_failover_and_applies_cooldown() {
        let (host, task) = mock("429 Too Many Requests", "Retry-After: 60\r\n", "{}").await;
        let client = RadioBrowser::new(reqwest::Client::new(), Some(host));
        client.hosts.lock().await.push("http://127.0.0.1:1".into());
        assert!(
            client
                .search("jazz", Sort::Name, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("rate-limiting")
        );
        task.await.unwrap();
        assert!(
            client
                .search("other", Sort::Name, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("retry in")
        );
    }

    #[tokio::test]
    #[ignore = "requires the public Radio Browser API"]
    async fn live_search() {
        let client = reqwest::Client::builder()
            .user_agent("interstellarfm-cli/0.1.0")
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap();
        let radio = RadioBrowser::new(client, None);
        let stations = radio.search("jazz", Sort::default(), false).await.unwrap();
        assert!(!stations.is_empty());
        assert!(stations.iter().all(|s| stream_url(&s.url).is_ok()));
        assert!(stations.iter().any(|s| !s.country_code.is_empty()));
        assert!(stations.iter().all(|s| s.legacy_country.is_empty()));
    }
}
