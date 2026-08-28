use std::collections::HashMap;

use scryer_plugin_pdk::*;
use scryer_plugin_sdk::current_sdk_constraint;
use scryer_plugin_sdk::{
    ConfigFieldDef, ConfigFieldRole, ConfigFieldType, IndexerCapabilities as Capabilities,
    IndexerCategoryModel, IndexerCategoryValueKind, IndexerDescriptor, IndexerFeedMode,
    IndexerLimitCapabilities, IndexerProtocol, IndexerResponseFeatures, IndexerSearchInput,
    IndexerSourceKind, IndexerTorrentCapabilities, PluginDescriptor,
    PluginSearchRequest as SearchRequest, PluginSearchResponse as SearchResponse,
    PluginSearchResult as SearchResult, ProviderDescriptor, SDK_VERSION,
};
use serde::Deserialize;

const DEFAULT_API_URL: &str = "https://movies-api.accel.li";
const DEFAULT_SITE_URL: &str = "https://yts.gg";
const USER_AGENT: &str = "Scryer YTS Indexer/0.1";
/// The API answers an empty list above this, so the page size is not a preference.
const PAGE_SIZE: usize = 50;
/// Domains YTS bakes into the URLs it returns, rewritten to the configured site.
const YTS_HOSTS: &[&str] = &["yts.mx", "yts.lt", "yts.bz", "yts.gg", "yts.am", "yts.ag"];

fn build_descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: "yts".to_string(),
        name: "YTS Indexer".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: SDK_VERSION.to_string(),
        sdk_constraint: current_sdk_constraint(),
        socket_permissions: vec![],
        provider: ProviderDescriptor::Indexer(IndexerDescriptor {
            provider_type: "yts".to_string(),
            provider_aliases: vec!["yify".to_string()],
            search_semantics_version: None,
            source_kind: IndexerSourceKind::Torrent,
            capabilities: Capabilities {
                supported_ids: HashMap::from([("movie".to_string(), vec!["imdb_id".to_string()])]),
                deduplicates_aliases: false,
                season_param: None,
                episode_param: None,
                query_param: Some("query_term".to_string()),
                supported_query_facets: vec!["movie".to_string()],
                search: true,
                imdb_search: true,
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
                    IndexerSearchInput::IdQuery,
                    IndexerSearchInput::Limit,
                ],
                supported_external_ids: vec!["imdb_id".to_string()],
                category_model: Some(IndexerCategoryModel {
                    value_kinds: vec![IndexerCategoryValueKind::Numeric],
                    provider_category_metadata: true,
                    ..IndexerCategoryModel::default()
                }),
                limits: Some(IndexerLimitCapabilities {
                    page_size: Some(PAGE_SIZE as u32),
                    max_page_size: Some(PAGE_SIZE as u32),
                    rate_limit_hint_seconds: Some(3),
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
            rate_limit_seconds: Some(3),
        }),
    }
}

fn search(req: SearchRequest) -> FnResult<SearchResponse> {
    let config = YtsConfig::from_host();
    let body = get_json(&search_url(&config, &req))?;
    let results = parse_movies(&config, &body)?;

    let limit = if req.limit == 0 {
        PAGE_SIZE
    } else {
        req.limit.min(PAGE_SIZE)
    };
    Ok(SearchResponse {
        results: results.into_iter().take(limit).collect(),
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
            Some("YTS v2 JSON API URL used for searching"),
        ),
        field(
            "site_url",
            "Website URL",
            ConfigFieldType::String,
            false,
            Some(DEFAULT_SITE_URL),
            Some("Site URL that replaces the domain YTS bakes into the URLs it returns"),
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

fn search_url(config: &YtsConfig, req: &SearchRequest) -> String {
    format!(
        "{}/api/v2/list_movies.json?query_term={}&limit={PAGE_SIZE}&sort_by=date_added&order_by=desc",
        config.api_url.trim_end_matches('/'),
        urlencoding::encode(&query_term(req))
    )
}

/// YTS takes one free-text term, and an IMDb id is the most precise one it accepts.
fn query_term(req: &SearchRequest) -> String {
    req.ids
        .get("imdb_id")
        .map(|imdb_id| imdb_id.trim())
        .filter(|imdb_id| !imdb_id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| prepare_query(&req.query))
}

/// Punctuation confuses the YTS matcher, so every run of it becomes one space.
fn prepare_query(raw: &str) -> String {
    let mut prepared = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            prepared.push(ch);
        } else if !prepared.ends_with(' ') {
            prepared.push(' ');
        }
    }
    prepared.trim().to_string()
}

fn get_json(url: &str) -> Result<String, Error> {
    let request = HttpRequest::new(url)
        .with_header("Accept", "application/json")
        .with_header("User-Agent", USER_AGENT);
    let response = http::request::<Vec<u8>>(&request, None)
        .map_err(|error| Error::msg(format!("YTS request failed: {error}")))?;
    let status = response.status_code();
    if status != 200 {
        return Err(Error::msg(format!("YTS API returned HTTP {status}")));
    }
    Ok(String::from_utf8_lossy(&response.body()).to_string())
}

fn parse_movies(config: &YtsConfig, body: &str) -> Result<Vec<SearchResult>, Error> {
    let response: ApiResponse = serde_json::from_str(body)
        .map_err(|error| Error::msg(format!("YTS JSON parse failed: {error}")))?;
    Ok(response
        .data
        .movies
        .iter()
        .flat_map(|movie| {
            movie
                .torrents
                .iter()
                .map(|torrent| to_search_result(config, movie, torrent))
        })
        .collect())
}

fn to_search_result(config: &YtsConfig, movie: &ApiMovie, torrent: &ApiTorrent) -> SearchResult {
    let title = release_title(movie, torrent);
    let info_hash = torrent.hash.trim().to_ascii_lowercase();
    let magnet_url = magnet_uri(&info_hash, &title);

    let mut external_ids = HashMap::new();
    if let Some(imdb_id) = normalize_imdb(movie.imdb_code.as_deref()) {
        external_ids.insert("imdb_id".to_string(), imdb_id);
    }

    let mut provider_extra = HashMap::new();
    provider_extra.insert(
        "quality".to_string(),
        serde_json::Value::from(torrent.quality.as_str()),
    );
    provider_extra.insert(
        "source_type".to_string(),
        serde_json::Value::from(torrent.torrent_type.as_str()),
    );
    if let Some(poster) = movie.large_cover_image.as_deref() {
        provider_extra.insert(
            "poster".to_string(),
            serde_json::Value::from(config.rewrite_host(poster)),
        );
    }

    SearchResult {
        title,
        download_url: Some(config.rewrite_host(&torrent.url)),
        size_bytes: torrent.size_bytes,
        published_at: torrent
            .date_uploaded_unix
            .filter(|uploaded| *uploaded > 0)
            .map(format_unix_timestamp),
        provider_extra,
        guid: Some(format!("YTS-{info_hash}")),
        info_url: movie.url.as_deref().map(|url| config.rewrite_host(url)),
        source_kind: Some(IndexerSourceKind::Torrent),
        protocol: Some(IndexerProtocol::Torrent),
        external_ids,
        categories: vec!["movie".to_string()],
        provider_categories: vec![provider_category(&torrent.quality).to_string()],
        magnet_url: Some(magnet_url),
        info_hash_v1: Some(info_hash),
        seeders: torrent.seeds,
        peers: torrent
            .seeds
            .zip(torrent.peers)
            .map(|(seeds, peers)| seeds + peers),
        leechers: torrent.peers,
        resolution: Some(torrent.quality.clone()),
        codec: torrent.video_codec.clone(),
        download_volume_factor: Some(0.0),
        upload_volume_factor: Some(1.0),
        ..SearchResult::default()
    }
}

/// Build the release name YTS itself never publishes.
///
/// The API describes a release as a set of fields rather than as a title, so
/// every consumer has to assemble one; this is the same assembly the Prowlarr
/// definition does, which is what release parsers have been trained on.
fn release_title(movie: &ApiMovie, torrent: &ApiTorrent) -> String {
    let base = movie
        .title_long
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match movie.year {
            Some(year) => format!("{} ({year})", movie.title),
            None => movie.title.clone(),
        });

    let rip = if torrent.torrent_type.eq_ignore_ascii_case("web") {
        "WEBRip"
    } else {
        "BRRip"
    };
    let audio = if torrent.audio_channels.as_deref() == Some("5.1") {
        "5.1 "
    } else {
        ""
    };
    let depth = if torrent.bit_depth.as_deref() == Some("10") {
        "10Bit "
    } else {
        ""
    };
    let codec = torrent.video_codec.as_deref().unwrap_or_default();

    format!(
        "{} {} {rip} {audio}{depth}{codec} -YTS",
        base.replace(':', ""),
        torrent.quality
    )
}

/// The YTS API has no categories, so the quality stands in for one.
fn provider_category(quality: &str) -> &'static str {
    match quality {
        "1080p" => "44",
        "2160p" => "46",
        "3D" => "47",
        _ => "45",
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

fn normalize_imdb(value: Option<&str>) -> Option<String> {
    let digits = value?
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
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month, day)
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

struct YtsConfig {
    api_url: String,
    site_url: String,
}

impl YtsConfig {
    fn from_host() -> Self {
        Self {
            api_url: config_value("api_url").unwrap_or_else(|| DEFAULT_API_URL.to_string()),
            site_url: config_value("site_url").unwrap_or_else(|| DEFAULT_SITE_URL.to_string()),
        }
    }

    /// Point a URL the API returned at the site the user can actually reach.
    fn rewrite_host(&self, url: &str) -> String {
        for scheme in ["https://", "http://"] {
            for host in YTS_HOSTS {
                if let Some(rest) = url.strip_prefix(&format!("{scheme}{host}/")) {
                    return format!("{}/{rest}", self.site_url.trim_end_matches('/'));
                }
            }
        }
        url.to_string()
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    data: ApiData,
}

/// `movies` is absent, not empty, when nothing matched.
#[derive(Debug, Default, Deserialize)]
struct ApiData {
    #[serde(default)]
    movies: Vec<ApiMovie>,
}

#[derive(Debug, Deserialize)]
struct ApiMovie {
    title: String,
    #[serde(default)]
    title_long: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    imdb_code: Option<String>,
    #[serde(default)]
    large_cover_image: Option<String>,
    #[serde(default)]
    torrents: Vec<ApiTorrent>,
}

#[derive(Debug, Deserialize)]
struct ApiTorrent {
    url: String,
    hash: String,
    quality: String,
    #[serde(default, rename = "type")]
    torrent_type: String,
    #[serde(default)]
    video_codec: Option<String>,
    #[serde(default)]
    bit_depth: Option<String>,
    #[serde(default)]
    audio_channels: Option<String>,
    #[serde(default)]
    seeds: Option<i64>,
    #[serde(default)]
    peers: Option<i64>,
    #[serde(default)]
    size_bytes: Option<i64>,
    #[serde(default)]
    date_uploaded_unix: Option<i64>,
}

indexer_command_compat::scryer_indexer_main!(descriptor = build_descriptor, search = search,);

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"status":"ok","data":{"movie_count":1,"limit":50,"page_number":1,
        "movies":[{"id":38698,"url":"https://yts.gg/movies/the-matrix-resurrections-2021",
        "imdb_code":"tt10838180","title":"The Matrix Resurrections",
        "title_long":"The Matrix Resurrections (2021)","year":2021,
        "large_cover_image":"https://yts.gg/assets/images/cover.jpg","torrents":[
        {"url":"https://yts.gg/torrent/download/107FACDA1820DF8212022863FFFA19A971563595",
         "hash":"107FACDA1820DF8212022863FFFA19A971563595","quality":"720p","type":"bluray",
         "video_codec":"x264","bit_depth":"8","audio_channels":"2.0","seeds":24,"peers":4,
         "size_bytes":1428076626,"date_uploaded_unix":1645277920},
        {"url":"https://yts.gg/torrent/download/E9CC8CB56C01EDCB7E324A5B57A7B5D04520DD48",
         "hash":"E9CC8CB56C01EDCB7E324A5B57A7B5D04520DD48","quality":"1080p","type":"web",
         "video_codec":"x265","bit_depth":"10","audio_channels":"5.1","seeds":100,"peers":14,
         "size_bytes":2931315180,"date_uploaded_unix":1645282983}]}]}}"#;

    fn config() -> YtsConfig {
        YtsConfig {
            api_url: DEFAULT_API_URL.to_string(),
            site_url: DEFAULT_SITE_URL.to_string(),
        }
    }

    #[test]
    fn descriptor_offers_imdb_movie_search_over_torrents() {
        let descriptor = build_descriptor();
        assert_eq!(descriptor.sdk_version, SDK_VERSION);
        assert_eq!(descriptor.sdk_constraint, current_sdk_constraint());

        let ProviderDescriptor::Indexer(indexer) = descriptor.provider else {
            panic!("expected indexer descriptor");
        };

        assert!(indexer.capabilities.imdb_search);
        assert_eq!(
            indexer.capabilities.supported_ids.get("movie"),
            Some(&vec!["imdb_id".to_string()])
        );
        assert_eq!(
            indexer.capabilities.supported_query_facets,
            vec!["movie".to_string()]
        );
    }

    #[test]
    fn an_imdb_id_is_preferred_over_the_free_text_query() {
        let req = SearchRequest {
            query: "The Matrix Resurrections".to_string(),
            ids: HashMap::from([("imdb_id".to_string(), "tt10838180".to_string())]),
            ..SearchRequest::default()
        };

        assert_eq!(
            search_url(&config(), &req),
            "https://movies-api.accel.li/api/v2/list_movies.json?query_term=tt10838180&limit=50&sort_by=date_added&order_by=desc"
        );
    }

    #[test]
    fn free_text_punctuation_collapses_to_single_spaces() {
        let req = SearchRequest {
            query: "America's  Next: Top Model".to_string(),
            ..SearchRequest::default()
        };

        assert_eq!(
            search_url(&config(), &req),
            "https://movies-api.accel.li/api/v2/list_movies.json?query_term=America%20s%20Next%20Top%20Model&limit=50&sort_by=date_added&order_by=desc"
        );
    }

    #[test]
    fn every_torrent_of_a_movie_becomes_its_own_release() {
        let results = parse_movies(&config(), BODY).expect("body should parse");
        assert_eq!(results.len(), 2);

        let titles: Vec<&str> = results.iter().map(|result| result.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "The Matrix Resurrections (2021) 720p BRRip x264 -YTS",
                "The Matrix Resurrections (2021) 1080p WEBRip 5.1 10Bit x265 -YTS",
            ]
        );
    }

    #[test]
    fn a_release_carries_the_fields_scryer_selects_on() {
        let result = &parse_movies(&config(), BODY).unwrap()[1];

        assert_eq!(result.size_bytes, Some(2_931_315_180));
        assert_eq!(result.seeders, Some(100));
        assert_eq!(result.leechers, Some(14));
        assert_eq!(result.peers, Some(114));
        assert_eq!(result.resolution.as_deref(), Some("1080p"));
        assert_eq!(result.codec.as_deref(), Some("x265"));
        assert_eq!(result.published_at.as_deref(), Some("2022-02-19T15:03:03Z"));
        assert_eq!(result.provider_categories, vec!["44".to_string()]);
        assert_eq!(result.categories, vec!["movie".to_string()]);
        assert_eq!(
            result.external_ids.get("imdb_id").map(String::as_str),
            Some("tt10838180")
        );
        assert_eq!(
            result.info_hash_v1.as_deref(),
            Some("e9cc8cb56c01edcb7e324a5b57a7b5d04520dd48")
        );
        assert!(result.magnet_url.as_deref().is_some_and(|magnet| {
            magnet.starts_with("magnet:?xt=urn:btih:e9cc8cb56c01edcb7e324a5b57a7b5d04520dd48&dn=")
        }));
    }

    #[test]
    fn returned_urls_move_to_the_configured_site() {
        let mut config = config();
        config.site_url = "https://yts.torrentbay.st/".to_string();
        let result = &parse_movies(&config, BODY).unwrap()[0];

        assert_eq!(
            result.download_url.as_deref(),
            Some(
                "https://yts.torrentbay.st/torrent/download/107FACDA1820DF8212022863FFFA19A971563595"
            )
        );
        assert_eq!(
            result.info_url.as_deref(),
            Some("https://yts.torrentbay.st/movies/the-matrix-resurrections-2021")
        );
        assert_eq!(
            result
                .provider_extra
                .get("poster")
                .and_then(|value| value.as_str()),
            Some("https://yts.torrentbay.st/assets/images/cover.jpg")
        );
    }

    #[test]
    fn a_no_match_answer_omits_the_movie_list_entirely() {
        let body = r#"{"status":"ok","data":{"movie_count":0,"limit":50,"page_number":1}}"#;

        assert!(parse_movies(&config(), body).unwrap().is_empty());
    }

    #[test]
    fn a_movie_without_a_long_title_still_gets_a_year() {
        let body = r#"{"data":{"movies":[{"title":"Nosferatu","year":1922,"torrents":[
            {"url":"https://yts.gg/torrent/download/AB","hash":"AB","quality":"1080p",
             "type":"bluray","video_codec":"x264"}]}]}}"#;

        assert_eq!(
            parse_movies(&config(), body).unwrap()[0].title,
            "Nosferatu (1922) 1080p BRRip x264 -YTS"
        );
    }
}
