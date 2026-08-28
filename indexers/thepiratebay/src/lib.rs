use std::collections::HashMap;

use scryer_plugin_pdk::*;
use scryer_plugin_sdk::current_sdk_constraint;
use scryer_plugin_sdk::{
    ConfigFieldDef, ConfigFieldOption, ConfigFieldRole, ConfigFieldType,
    IndexerCapabilities as Capabilities, IndexerCategoryModel, IndexerCategoryValueKind,
    IndexerDescriptor, IndexerFeedMode, IndexerLimitCapabilities, IndexerProtocol,
    IndexerResponseFeatures, IndexerSearchInput, IndexerSourceKind, IndexerTorrentCapabilities,
    PluginDescriptor, PluginSearchRequest as SearchRequest, PluginSearchResponse as SearchResponse,
    PluginSearchResult as SearchResult, ProviderDescriptor, SDK_VERSION,
};
use serde::Deserialize;

const DEFAULT_API_URL: &str = "https://apibay.org";
const DEFAULT_SITE_URL: &str = "https://thepiratebay.org";
const DEFAULT_TOP100: &str = "recent";
/// The precompiled feeds apibay publishes, as `(feed, label)`.
const TOP100_FEEDS: &[(&str, &str)] = &[
    ("recent", "All"),
    ("100", "Audio"),
    ("200", "Movies/TV"),
    ("300", "Apps"),
    ("400", "Games"),
    ("500", "XXX"),
    ("600", "Books"),
];
const USER_AGENT: &str = "Scryer The Pirate Bay Indexer/0.1";
const PAGE_SIZE: usize = 100;

fn build_descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: "thepiratebay".to_string(),
        name: "The Pirate Bay Indexer".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: SDK_VERSION.to_string(),
        sdk_constraint: current_sdk_constraint(),
        socket_permissions: vec![],
        provider: ProviderDescriptor::Indexer(IndexerDescriptor {
            provider_type: "thepiratebay".to_string(),
            provider_aliases: vec!["tpb".to_string(), "apibay".to_string()],
            search_semantics_version: None,
            source_kind: IndexerSourceKind::Torrent,
            capabilities: Capabilities {
                // apibay only accepts free text; the IMDb id it reports back is
                // result metadata, not a search key.
                supported_ids: HashMap::new(),
                deduplicates_aliases: false,
                season_param: None,
                episode_param: None,
                query_param: Some("q".to_string()),
                supported_query_facets: vec![
                    "movie".to_string(),
                    "series".to_string(),
                    "anime".to_string(),
                ],
                search: true,
                imdb_search: false,
                tvdb_search: false,
                anidb_search: false,
                rss: true,
                protocols: vec![IndexerProtocol::Torrent],
                feed_modes: vec![
                    IndexerFeedMode::Recent,
                    IndexerFeedMode::Rss,
                    IndexerFeedMode::AutomaticSearch,
                    IndexerFeedMode::InteractiveSearch,
                ],
                search_inputs: vec![
                    IndexerSearchInput::TextQuery,
                    IndexerSearchInput::Season,
                    IndexerSearchInput::Episode,
                    IndexerSearchInput::Category,
                    IndexerSearchInput::Limit,
                ],
                supported_external_ids: vec![],
                category_model: Some(IndexerCategoryModel {
                    value_kinds: vec![IndexerCategoryValueKind::Numeric],
                    provider_category_metadata: true,
                    ..IndexerCategoryModel::default()
                }),
                limits: Some(IndexerLimitCapabilities {
                    page_size: Some(PAGE_SIZE as u32),
                    max_page_size: Some(PAGE_SIZE as u32),
                    rate_limit_hint_seconds: Some(2),
                    ..IndexerLimitCapabilities::default()
                }),
                torrent: Some(IndexerTorrentCapabilities {
                    reports_seeders: true,
                    reports_peers: true,
                    reports_info_hash: true,
                    reports_magnet_uri: true,
                    supports_private_tracker_flags: false,
                    ..IndexerTorrentCapabilities::default()
                }),
                response_features: Some(IndexerResponseFeatures {
                    info_url: true,
                    guid: true,
                    raw_provider_metadata: true,
                    ..IndexerResponseFeatures::default()
                }),
            },
            scoring_policies: vec![],
            config_fields: config_fields(),
            allowed_hosts: vec![],
            rate_limit_seconds: Some(2),
        }),
    }
}

fn search(req: SearchRequest) -> FnResult<SearchResponse> {
    let config = PirateBayConfig::from_host();
    let mut results = Vec::new();

    for url in search_urls(&config, &req) {
        let body = get_json(&url)?;
        results.append(&mut parse_torrents(&config, &body)?);
    }

    let limit = if req.limit == 0 {
        PAGE_SIZE
    } else {
        req.limit.min(PAGE_SIZE)
    };
    let results = dedupe_results(results).into_iter().take(limit).collect();
    Ok(SearchResponse {
        results,
        ..Default::default()
    })
}

fn config_fields() -> Vec<ConfigFieldDef> {
    vec![
        connection_field(
            "api_url",
            "API URL",
            true,
            Some(DEFAULT_API_URL),
            Some("apibay JSON API URL used for searching"),
        ),
        field(
            "site_url",
            "Website URL",
            ConfigFieldType::String,
            false,
            Some(DEFAULT_SITE_URL),
            Some("Site URL used to build the details link of a release"),
        ),
        ConfigFieldDef {
            options: TOP100_FEEDS
                .iter()
                .map(|(value, label)| ConfigFieldOption {
                    value: value.to_string(),
                    label: label.to_string(),
                })
                .collect(),
            ..field(
                "top100",
                "Top 100 Feed",
                ConfigFieldType::Select,
                false,
                Some(DEFAULT_TOP100),
                Some(
                    "Feed used for keyword-less searches. The recent feed is mostly video, so pick the group that matches what this indexer is used for",
                ),
            )
        },
        field(
            "uploader",
            "Filter by Uploader",
            ConfigFieldType::String,
            false,
            None,
            Some("Case-sensitive uploader username, or empty for every uploader"),
        ),
        field(
            "minimum_seeders",
            "Minimum Seeders",
            ConfigFieldType::Number,
            false,
            Some("1"),
            Some("Minimum seeders preference for host-side release decisions"),
        ),
    ]
}

fn search_urls(config: &PirateBayConfig, req: &SearchRequest) -> Vec<String> {
    let api_url = config.api_url.trim_end_matches('/');
    let terms = search_terms(req);
    if terms.is_empty() {
        return vec![format!(
            "{api_url}/precompiled/data_top100_{}.json",
            config.top100
        )];
    }

    let categories = request_categories(req);
    terms
        .into_iter()
        .map(|term| {
            format!(
                "{api_url}/q.php?q={}&cat={categories}",
                urlencoding::encode(&term)
            )
        })
        .collect()
}

fn search_terms(req: &SearchRequest) -> Vec<String> {
    let suffix = episode_suffix(req);
    let terms = search_titles(req)
        .iter()
        .map(|title| prepare_query(&format!("{title}{suffix}")))
        .filter(|term| !term.is_empty())
        .collect();
    dedupe(terms)
}

fn search_titles(req: &SearchRequest) -> Vec<String> {
    let mut titles = Vec::new();
    if !req.query.trim().is_empty() {
        titles.push(req.query.trim().to_string());
    }
    for alias in &req.tagged_aliases {
        if !alias.name.trim().is_empty() {
            titles.push(alias.name.trim().to_string());
        }
    }
    dedupe(titles)
}

fn episode_suffix(req: &SearchRequest) -> String {
    match (req.season, req.episode) {
        (Some(season), Some(episode)) if season > 0 && episode > 0 => {
            format!(" S{season:02}E{episode:02}")
        }
        (Some(season), None) if season > 0 => format!(" S{season:02}"),
        _ => String::new(),
    }
}

/// Reduce a title to the dotted, lower-case form TPB's search engine answers best.
///
/// TPB tokenises on word characters, so an apostrophe or a CJK ideogram inside
/// the query returns nothing at all rather than fewer rows. Both are collapsed
/// to the separator instead, matching the keyword filters Prowlarr had to add
/// for the same two failure reports.
fn prepare_query(raw: &str) -> String {
    let lowered = strip_word(&raw.to_lowercase(), "it's");
    let mut prepared = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if is_query_word_char(ch) {
            prepared.push(ch);
        } else if !prepared.ends_with('.') {
            prepared.push('.');
        }
    }
    prepared.trim_matches('.').to_string()
}

fn is_query_word_char(ch: char) -> bool {
    (ch.is_alphanumeric() || ch == '_') && !('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn strip_word(text: &str, word: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(word) {
        let (before, matched) = rest.split_at(position);
        let after = &matched[word.len()..];
        let bounded = before
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_alphanumeric())
            && after.chars().next().is_none_or(|ch| !ch.is_alphanumeric());
        out.push_str(before);
        if !bounded {
            out.push_str(word);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn request_categories(req: &SearchRequest) -> String {
    let mut values: Vec<String> = Vec::new();
    for raw in req.category.iter().chain(req.categories.iter()) {
        let trimmed = raw.trim();
        if !trimmed.is_empty()
            && trimmed.chars().all(|ch| ch.is_ascii_digit())
            && !values.iter().any(|existing| existing == trimmed)
        {
            values.push(trimmed.to_string());
        }
    }
    values.join(",")
}

fn get_json(url: &str) -> Result<String, Error> {
    let request = HttpRequest::new(url)
        .with_header("Accept", "application/json")
        .with_header("User-Agent", USER_AGENT);
    let response = http::request::<Vec<u8>>(&request, None)
        .map_err(|error| Error::msg(format!("The Pirate Bay request failed: {error}")))?;
    let status = response.status_code();
    let body = String::from_utf8_lossy(&response.body()).to_string();
    if status != 200 {
        return Err(Error::msg(format!(
            "The Pirate Bay API returned HTTP {status}"
        )));
    }
    Ok(body)
}

fn parse_torrents(config: &PirateBayConfig, body: &str) -> Result<Vec<SearchResult>, Error> {
    let torrents: Vec<ApiTorrent> = serde_json::from_str(body)
        .map_err(|error| Error::msg(format!("The Pirate Bay JSON parse failed: {error}")))?;
    Ok(torrents
        .iter()
        .filter(|torrent| !torrent.is_no_results_row())
        .filter(|torrent| config.accepts_uploader(torrent.username.as_deref()))
        .map(|torrent| to_search_result(config, torrent))
        .collect())
}

fn to_search_result(config: &PirateBayConfig, torrent: &ApiTorrent) -> SearchResult {
    let id = torrent.id.as_text();
    let title = normalize_title(&torrent.name);
    let info_hash = torrent.info_hash.trim().to_ascii_lowercase();
    let seeders = torrent.seeders.as_ref().and_then(Scalar::as_i64);
    let leechers = torrent.leechers.as_ref().and_then(Scalar::as_i64);
    let category = torrent.category.as_ref().and_then(Scalar::as_i64);

    let mut external_ids = HashMap::new();
    if let Some(imdb_id) = torrent
        .imdb
        .as_ref()
        .map(Scalar::as_text)
        .and_then(normalize_imdb)
    {
        external_ids.insert("imdb_id".to_string(), imdb_id);
    }

    let mut provider_extra = HashMap::new();
    if let Some(username) = torrent
        .username
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        provider_extra.insert("username".to_string(), serde_json::Value::from(username));
    }
    if let Some(status) = torrent.status.as_deref().filter(|value| !value.is_empty()) {
        provider_extra.insert("status".to_string(), serde_json::Value::from(status));
    }
    if let Some(files) = torrent.num_files.as_ref().and_then(Scalar::as_i64) {
        provider_extra.insert("num_files".to_string(), serde_json::Value::from(files));
    }

    SearchResult {
        title: title.clone(),
        size_bytes: torrent.size.as_ref().and_then(Scalar::as_i64),
        published_at: torrent
            .added
            .as_ref()
            .and_then(Scalar::as_i64)
            .filter(|added| *added > 0)
            .map(format_unix_timestamp),
        provider_extra,
        guid: Some(format!("TPB-{id}")),
        info_url: Some(format!(
            "{}/description.php?id={id}",
            config.site_url.trim_end_matches('/')
        )),
        source_kind: Some(IndexerSourceKind::Torrent),
        protocol: Some(IndexerProtocol::Torrent),
        external_ids,
        categories: category
            .and_then(scryer_category)
            .map(str::to_string)
            .into_iter()
            .collect(),
        provider_categories: category
            .map(|value| value.to_string())
            .into_iter()
            .collect(),
        magnet_url: Some(magnet_uri(&info_hash, &title)),
        info_hash_v1: Some(info_hash),
        seeders,
        peers: seeders
            .zip(leechers)
            .map(|(seeders, leechers)| seeders + leechers),
        leechers,
        download_volume_factor: Some(0.0),
        upload_volume_factor: Some(1.0),
        ..SearchResult::default()
    }
}

/// Rewrite the two title shapes TPB uploaders use that a release parser cannot read.
///
/// `Season 4` and a spaced ` - GROUP` suffix both defeat season and release-group
/// detection, and TPB is inconsistent enough about them that fixing it at the
/// indexer is cheaper than teaching every consumer.
fn normalize_title(raw: &str) -> String {
    tighten_group_suffix(&normalize_season_markers(raw.trim()))
}

fn normalize_season_markers(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let mut out = String::with_capacity(title.len());
    let mut index = 0;
    while index < chars.len() {
        match match_season_marker(&chars, index) {
            Some((season, end)) => {
                out.push_str(&format!("S{season:02}"));
                index = end;
            }
            None => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }
    out
}

fn match_season_marker(chars: &[char], start: usize) -> Option<(u32, usize)> {
    const MARKER: &[char] = &['s', 'e', 'a', 's', 'o', 'n'];

    if start > 0 && chars[start - 1].is_alphanumeric() {
        return None;
    }
    if chars.len() < start + MARKER.len() {
        return None;
    }
    if !chars[start..start + MARKER.len()]
        .iter()
        .zip(MARKER)
        .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
    {
        return None;
    }

    let mut index = skip_separators(chars, start + MARKER.len())?;
    let digits_start = index;
    while index < chars.len() && index - digits_start < 2 && chars[index].is_ascii_digit() {
        index += 1;
    }
    if index == digits_start {
        return None;
    }
    let season: u32 = chars[digits_start..index]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;

    if let Some(after_separator) = skip_separators(chars, index)
        && let Some(after_complete) = match_word(chars, after_separator, "complete")
    {
        index = after_complete;
    }
    Some((season, index))
}

fn skip_separators(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    while index < chars.len() && (chars[index].is_whitespace() || chars[index] == '.') {
        index += 1;
    }
    (index > start).then_some(index)
}

fn match_word(chars: &[char], start: usize, word: &str) -> Option<usize> {
    let word: Vec<char> = word.chars().collect();
    let end = start + word.len();
    if end > chars.len() {
        return None;
    }
    if end < chars.len() && chars[end].is_alphanumeric() {
        return None;
    }
    chars[start..end]
        .iter()
        .zip(&word)
        .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        .then_some(end)
}

fn tighten_group_suffix(title: &str) -> String {
    let Some(position) = title.rfind("- ") else {
        return title.to_string();
    };
    let suffix = &title[position + 2..];
    if !is_group_token(suffix) {
        return title.to_string();
    }
    format!("{}-{suffix}", &title[..position])
}

fn is_group_token(value: &str) -> bool {
    if value.is_empty() || value.starts_with('-') || value.ends_with('-') {
        return false;
    }
    let mut hyphens = 0;
    for ch in value.chars() {
        if ch == '-' {
            hyphens += 1;
            if hyphens > 1 {
                return false;
            }
        } else if !(ch.is_alphanumeric() || ch == '_') {
            return false;
        }
    }
    true
}

fn scryer_category(category: i64) -> Option<&'static str> {
    match category {
        201..=204 | 207 | 209..=211 | 299 => Some("movie"),
        205 | 206 | 208 | 212 => Some("series"),
        _ => None,
    }
}

fn magnet_uri(info_hash: &str, title: &str) -> String {
    const TRACKERS: &[&str] = &[
        "udp://tracker.opentrackr.org:1337/announce",
        "udp://open.stealth.si:80/announce",
        "udp://tracker.torrent.eu.org:451/announce",
        "udp://open.demonii.com:1337/announce",
        "udp://exodus.desync.com:6969/announce",
    ];

    let mut uri = format!(
        "magnet:?xt=urn:btih:{info_hash}&dn={}",
        urlencoding::encode(title)
    );
    for tracker in TRACKERS {
        uri.push_str("&tr=");
        uri.push_str(&urlencoding::encode(tracker));
    }
    uri
}

fn normalize_imdb(value: String) -> Option<String> {
    let digits = value
        .trim()
        .trim_start_matches("tt")
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| format!("tt{digits:0>7}"))
}

fn format_unix_timestamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (year + i64::from(month <= 2), month, day)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

fn dedupe_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut out: Vec<SearchResult> = Vec::new();
    for result in results {
        let key = result.guid.clone().unwrap_or_else(|| result.title.clone());
        if !out
            .iter()
            .any(|existing| existing.guid.as_ref().unwrap_or(&existing.title) == &key)
        {
            out.push(result);
        }
    }
    out
}

fn config_value(key: &str) -> Option<String> {
    config::get(key)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn field(
    key: &str,
    label: &str,
    field_type: ConfigFieldType,
    required: bool,
    default_value: Option<&str>,
    help_text: Option<&str>,
) -> ConfigFieldDef {
    ConfigFieldDef {
        key: key.to_string(),
        label: label.to_string(),
        field_type,
        required,
        default_value: default_value.map(str::to_string),
        value_source: Default::default(),
        role: None,
        host_binding: None,
        options: vec![],
        help_text: help_text.map(str::to_string),
    }
}

fn connection_field(
    key: &str,
    label: &str,
    required: bool,
    default_value: Option<&str>,
    help_text: Option<&str>,
) -> ConfigFieldDef {
    ConfigFieldDef {
        role: Some(ConfigFieldRole::ConnectionUrl),
        ..field(
            key,
            label,
            ConfigFieldType::String,
            required,
            default_value,
            help_text,
        )
    }
}

struct PirateBayConfig {
    api_url: String,
    site_url: String,
    top100: String,
    uploader: Option<String>,
}

impl PirateBayConfig {
    fn from_host() -> Self {
        Self {
            api_url: config_value("api_url").unwrap_or_else(|| DEFAULT_API_URL.to_string()),
            site_url: config_value("site_url").unwrap_or_else(|| DEFAULT_SITE_URL.to_string()),
            // The value lands in a URL path, so anything but a known feed name
            // falls back rather than building a request nobody can read.
            top100: config_value("top100")
                .filter(|value| TOP100_FEEDS.iter().any(|(feed, _)| feed == value))
                .unwrap_or_else(|| DEFAULT_TOP100.to_string()),
            uploader: config_value("uploader"),
        }
    }

    fn accepts_uploader(&self, username: Option<&str>) -> bool {
        match &self.uploader {
            Some(uploader) => username == Some(uploader.as_str()),
            None => true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Scalar {
    Text(String),
    Number(i64),
}

impl Scalar {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Text(value) => value.trim().parse().ok(),
            Self::Number(value) => Some(*value),
        }
    }

    fn as_text(&self) -> String {
        match self {
            Self::Text(value) => value.trim().to_string(),
            Self::Number(value) => value.to_string(),
        }
    }
}

/// One apibay row.
///
/// `q.php` answers with every number quoted while the precompiled top-100 feeds
/// answer with real JSON numbers, so each of those fields has to accept both.
#[derive(Debug, Deserialize)]
struct ApiTorrent {
    id: Scalar,
    name: String,
    info_hash: String,
    #[serde(default)]
    seeders: Option<Scalar>,
    #[serde(default)]
    leechers: Option<Scalar>,
    #[serde(default)]
    num_files: Option<Scalar>,
    #[serde(default)]
    size: Option<Scalar>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    added: Option<Scalar>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<Scalar>,
    #[serde(default)]
    imdb: Option<Scalar>,
}

impl ApiTorrent {
    /// apibay reports "nothing found" as a single row with id 0, not an empty array.
    fn is_no_results_row(&self) -> bool {
        self.id.as_i64() == Some(0) || self.info_hash.chars().all(|ch| ch == '0')
    }
}

indexer_command_compat::scryer_indexer_main!(descriptor = build_descriptor, search = search,);

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PirateBayConfig {
        PirateBayConfig {
            api_url: DEFAULT_API_URL.to_string(),
            site_url: DEFAULT_SITE_URL.to_string(),
            top100: DEFAULT_TOP100.to_string(),
            uploader: None,
        }
    }

    #[test]
    fn descriptor_is_text_only_and_torrent_shaped() {
        let descriptor = build_descriptor();
        assert_eq!(descriptor.sdk_version, SDK_VERSION);
        assert_eq!(descriptor.sdk_constraint, current_sdk_constraint());

        let ProviderDescriptor::Indexer(indexer) = descriptor.provider else {
            panic!("expected indexer descriptor");
        };

        assert!(indexer.capabilities.supported_ids.is_empty());
        assert!(!indexer.capabilities.imdb_search);
        assert_eq!(indexer.capabilities.query_param.as_deref(), Some("q"));
        assert!(
            indexer
                .capabilities
                .torrent
                .is_some_and(|torrent| torrent.reports_magnet_uri)
        );
    }

    #[test]
    fn keyword_search_uses_the_dotted_lowercase_query_form() {
        let req = SearchRequest {
            query: "The Matrix".to_string(),
            categories: vec!["207".to_string(), "211".to_string()],
            ..SearchRequest::default()
        };

        assert_eq!(
            search_urls(&config(), &req),
            vec!["https://apibay.org/q.php?q=the.matrix&cat=207,211"]
        );
    }

    #[test]
    fn episode_search_appends_the_season_episode_token() {
        let req = SearchRequest {
            query: "Severance".to_string(),
            season: Some(2),
            episode: Some(7),
            ..SearchRequest::default()
        };

        assert_eq!(
            search_urls(&config(), &req),
            vec!["https://apibay.org/q.php?q=severance.s02e07&cat="]
        );
    }

    #[test]
    fn keywordless_search_falls_back_to_the_configured_top100_feed() {
        let mut config = config();
        config.top100 = "200".to_string();

        assert_eq!(
            search_urls(&config, &SearchRequest::default()),
            vec!["https://apibay.org/precompiled/data_top100_200.json"]
        );
    }

    #[test]
    fn query_preparation_drops_the_tokens_tpb_cannot_match() {
        assert_eq!(prepare_query("It's Always Sunny"), "always.sunny");
        // Only the standalone word goes; the same letters inside one do not.
        assert_eq!(prepare_query("Fits Alright"), "fits.alright");
        assert_eq!(prepare_query("鬼滅之刃 Mugen Train"), "mugen.train");
    }

    #[test]
    fn quoted_and_numeric_rows_both_parse() {
        let quoted = r#"[{"id":"7349687","name":"The Matrix (1999) 1080p BrRip x264 - YIFY",
            "info_hash":"D7A46713EAEE18C746B3254B7D1492A50FD9D6CE","leechers":"108","seeders":"789",
            "num_files":"6","size":"1992277407","username":"YIFY","added":"1339543961",
            "status":"vip","category":"207","imdb":"tt0133093"}]"#;
        let numeric = r#"[{"id":7349687,"info_hash":"D7A46713EAEE18C746B3254B7D1492A50FD9D6CE",
            "category":207,"name":"The Matrix (1999) 1080p BrRip x264 - YIFY","status":"vip",
            "num_files":6,"size":1992277407,"seeders":789,"leechers":108,"username":"YIFY",
            "added":1339543961,"anon":0,"imdb":null}]"#;

        for body in [quoted, numeric] {
            let results = parse_torrents(&config(), body).expect("row should parse");
            let result = results.first().expect("one release");
            assert_eq!(result.title, "The Matrix (1999) 1080p BrRip x264 -YIFY");
            assert_eq!(result.size_bytes, Some(1_992_277_407));
            assert_eq!(result.seeders, Some(789));
            assert_eq!(result.leechers, Some(108));
            assert_eq!(result.peers, Some(897));
            assert_eq!(result.published_at.as_deref(), Some("2012-06-12T23:32:41Z"));
            assert_eq!(result.guid.as_deref(), Some("TPB-7349687"));
            assert_eq!(
                result.info_url.as_deref(),
                Some("https://thepiratebay.org/description.php?id=7349687")
            );
            assert_eq!(result.categories, vec!["movie".to_string()]);
            assert_eq!(result.provider_categories, vec!["207".to_string()]);
            assert_eq!(
                result.info_hash_v1.as_deref(),
                Some("d7a46713eaee18c746b3254b7d1492a50fd9d6ce")
            );
            assert!(
                result
                    .magnet_url
                    .as_deref()
                    .is_some_and(|magnet| magnet.starts_with(
                        "magnet:?xt=urn:btih:d7a46713eaee18c746b3254b7d1492a50fd9d6ce&dn="
                    ))
            );
        }

        // Only the quoted feed carries an IMDb id; the top-100 feed sends null.
        let quoted_result = &parse_torrents(&config(), quoted).unwrap()[0];
        assert_eq!(
            quoted_result
                .external_ids
                .get("imdb_id")
                .map(String::as_str),
            Some("tt0133093")
        );
        assert!(
            parse_torrents(&config(), numeric).unwrap()[0]
                .external_ids
                .is_empty()
        );
    }

    #[test]
    fn the_no_results_row_is_not_a_release() {
        let body = r#"[{"id":"0","name":"No results returned",
            "info_hash":"0000000000000000000000000000000000000000","leechers":"0","seeders":"0",
            "num_files":"0","size":"0","username":"","added":"0","status":"member",
            "category":"0","imdb":"","total_found":"1"}]"#;

        assert!(parse_torrents(&config(), body).unwrap().is_empty());
    }

    #[test]
    fn the_uploader_filter_is_case_sensitive() {
        let mut config = config();
        config.uploader = Some("YIFY".to_string());
        let body = r#"[{"id":"1","name":"A","info_hash":"aa","username":"YIFY"},
            {"id":"2","name":"B","info_hash":"bb","username":"yify"},
            {"id":"3","name":"C","info_hash":"cc"}]"#;

        let results = parse_torrents(&config, body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].guid.as_deref(), Some("TPB-1"));
    }

    #[test]
    fn season_words_become_season_tokens() {
        assert_eq!(normalize_title("Andor Season 2 1080p"), "Andor S02 1080p");
        assert_eq!(
            normalize_title("The.Wire.Season.12.Complete.720p"),
            "The.Wire.S12.720p"
        );
        assert_eq!(
            normalize_title("Seasoned Chef 1080p"),
            "Seasoned Chef 1080p"
        );
    }

    #[test]
    fn a_spaced_group_suffix_is_tightened() {
        assert_eq!(
            normalize_title("Show S01E01 1080p WEB - MeGusta"),
            "Show S01E01 1080p WEB -MeGusta"
        );
        // Only the trailing token is a group; the size note before it is left alone.
        assert_eq!(
            normalize_title("Movie 1080p BrRip x264 - 1.85GB - YIFY"),
            "Movie 1080p BrRip x264 - 1.85GB -YIFY"
        );
        assert_eq!(
            normalize_title("Movie 1080p - not a group"),
            "Movie 1080p - not a group"
        );
    }
}
