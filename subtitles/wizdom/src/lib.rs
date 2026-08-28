use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use extism_pdk::*;
use scryer_plugin_sdk::current_sdk_constraint;
use scryer_plugin_sdk::{
    ConfigFieldDef, ConfigFieldRole, ConfigFieldType, ConfigFieldValueSource, PluginDescriptor,
    PluginResult, ProviderDescriptor, SDK_VERSION, SubtitleCapabilities, SubtitleDescriptor,
    SubtitleMatchHint, SubtitleMatchHintKind, SubtitlePluginCandidate,
    SubtitlePluginDownloadRequest, SubtitlePluginDownloadResponse, SubtitlePluginSearchRequest,
    SubtitlePluginSearchResponse, SubtitlePluginValidateConfigRequest,
    SubtitlePluginValidateConfigResponse, SubtitleProviderMode, SubtitleQueryMediaKind,
    SubtitleValidateConfigStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROVIDER_ID: &str = "wizdom";
const PROVIDER_TYPE: &str = "wizdom";
const DEFAULT_BASE_URL: &str = "https://wizdom.xyz";
const TMDB_API_BASE: &str = "https://api.themoviedb.org/3";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION"));
const PROVIDER_LANGUAGE: &str = "heb";
const RETRY_AMOUNT: usize = 3;
const RETRY_TIMEOUT_SECS: u64 = 5;
const MAX_RATE_LIMIT_WAIT_SECONDS: i64 = 10;
const MAX_DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;
const ERROR_BODY_PREVIEW_LIMIT: usize = 240;
const VALIDATION_PROBE_IMDB_ID: &str = "tt1375666";

#[derive(Clone, Debug)]
struct WizdomConfig {
    base_url: String,
    tmdb_api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    InvalidConfig,
    AuthFailed,
    RateLimited,
    Unreachable,
    Unsupported,
    Provider,
}

#[derive(Debug, Clone)]
struct Failure {
    kind: FailureKind,
    message: String,
    retry_after_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WizdomDownloadRef {
    subtitle_id: String,
    filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_url: Option<String>,
}

/// One subtitle row from `api/releases/{imdb_id}`. `version` carries the
/// uploader's release name and is the only match signal the API exposes.
#[derive(Debug, Clone, Deserialize)]
struct WizdomSub {
    id: Value,
    #[serde(default)]
    version: String,
}

#[derive(Debug, Default, Deserialize)]
struct WizdomReleases {
    #[serde(default)]
    subs: Option<Value>,
}

#[derive(Debug)]
struct DownloadArtifact {
    bytes: Vec<u8>,
    content_type: Option<String>,
    filename: Option<String>,
}

#[plugin_fn]
pub fn scryer_describe(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&build_descriptor())?)
}

#[plugin_fn]
pub fn scryer_validate_config(input: String) -> FnResult<String> {
    let _: SubtitlePluginValidateConfigRequest = serde_json::from_str(&input)?;
    let response = match WizdomConfig::from_extism() {
        Ok(config) => match validate_config_impl(&config) {
            Ok(()) => SubtitlePluginValidateConfigResponse {
                status: SubtitleValidateConfigStatus::Valid,
                message: None,
                retry_after_seconds: None,
            },
            Err(failure) => validation_error_response(&failure),
        },
        Err(failure) => validation_error_response(&failure),
    };

    Ok(serde_json::to_string(&PluginResult::Ok(response))?)
}

#[plugin_fn]
pub fn scryer_subtitle_search(input: String) -> FnResult<String> {
    let request: SubtitlePluginSearchRequest = serde_json::from_str(&input)?;
    let config = WizdomConfig::from_extism().map_err(|failure| Error::msg(failure.message))?;
    let results =
        search_subtitles_impl(&config, &request).map_err(|failure| Error::msg(failure.message))?;
    Ok(serde_json::to_string(&PluginResult::Ok(
        SubtitlePluginSearchResponse { results },
    ))?)
}

#[plugin_fn]
pub fn scryer_subtitle_download(input: String) -> FnResult<String> {
    let request: SubtitlePluginDownloadRequest = serde_json::from_str(&input)?;
    let config = WizdomConfig::from_extism().map_err(|failure| Error::msg(failure.message))?;
    let reference: WizdomDownloadRef =
        serde_json::from_str(&request.provider_file_id).map_err(Error::msg)?;
    let response = download_subtitle_impl(&config, &reference)
        .map_err(|failure| Error::msg(failure.message))?;
    Ok(serde_json::to_string(&PluginResult::Ok(response))?)
}

fn build_descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PROVIDER_ID.to_string(),
        name: "Wizdom".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: SDK_VERSION.to_string(),
        sdk_constraint: current_sdk_constraint(),
        socket_permissions: vec![],
        provider: ProviderDescriptor::Subtitle(SubtitleDescriptor {
            provider_type: PROVIDER_TYPE.to_string(),
            provider_aliases: vec![],
            config_fields: config_fields(),
            default_base_url: Some(DEFAULT_BASE_URL.to_string()),
            allowed_hosts: vec!["wizdom.xyz".to_string(), "api.themoviedb.org".to_string()],
            capabilities: SubtitleCapabilities {
                mode: SubtitleProviderMode::Catalog,
                supported_media_kinds: vec![
                    SubtitleQueryMediaKind::Movie,
                    SubtitleQueryMediaKind::Episode,
                ],
                recommended_facets: vec!["movie".to_string(), "series".to_string()],
                supports_hash_lookup: false,
                supports_forced: false,
                supports_hearing_impaired: false,
                supports_ai_translated: false,
                supports_machine_translated: false,
                supported_languages: vec![PROVIDER_LANGUAGE.to_string()],
                sync: None,
            },
        }),
    }
}

fn config_fields() -> Vec<ConfigFieldDef> {
    vec![
        ConfigFieldDef {
            key: "base_url".to_string(),
            label: "API URL".to_string(),
            field_type: ConfigFieldType::String,
            required: true,
            default_value: Some(DEFAULT_BASE_URL.to_string()),
            value_source: ConfigFieldValueSource::User,
            role: Some(ConfigFieldRole::ConnectionUrl),
            host_binding: None,
            options: vec![],
            help_text: Some("Wizdom API URL".to_string()),
        },
        ConfigFieldDef {
            key: "tmdb_api_key".to_string(),
            label: "TMDB API Key".to_string(),
            field_type: ConfigFieldType::Password,
            required: false,
            default_value: None,
            value_source: ConfigFieldValueSource::User,
            role: None,
            host_binding: None,
            options: vec![],
            help_text: Some(
                "Optional TMDB key used to resolve an IMDb ID by title when Scryer has none"
                    .to_string(),
            ),
        },
    ]
}

impl WizdomConfig {
    fn from_extism() -> Result<Self, Failure> {
        let base_url = config_string("base_url")?
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(Failure::new(
                FailureKind::InvalidConfig,
                "base_url must be an http or https URL",
            ));
        }
        Ok(Self {
            base_url,
            tmdb_api_key: config_string("tmdb_api_key")?,
        })
    }
}

impl Failure {
    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_after_seconds: None,
        }
    }

    fn with_retry_after(mut self, retry_after_seconds: Option<i64>) -> Self {
        self.retry_after_seconds = retry_after_seconds;
        self
    }
}

fn validate_config_impl(config: &WizdomConfig) -> Result<(), Failure> {
    fetch_releases(config, VALIDATION_PROBE_IMDB_ID).map(|_| ())
}

fn search_subtitles_impl(
    config: &WizdomConfig,
    request: &SubtitlePluginSearchRequest,
) -> Result<Vec<SubtitlePluginCandidate>, Failure> {
    if !is_searchable(request) {
        return Ok(Vec::new());
    }

    let Some(imdb_id) = resolve_imdb_id(config, request)? else {
        return Ok(Vec::new());
    };

    let releases = fetch_releases(config, &imdb_id)?;
    let subs = collect_subs(
        releases.subs.as_ref(),
        request.media_kind,
        request.season,
        request.episode,
    );

    Ok(subs
        .iter()
        .filter_map(|sub| sub_to_candidate(config, request, sub, &imdb_id))
        .collect())
}

/// Wizdom serves Hebrew only, and its episode rows are reachable only through
/// both a season and an episode key.
fn is_searchable(request: &SubtitlePluginSearchRequest) -> bool {
    if !requests_hebrew(request) {
        return false;
    }
    match request.media_kind {
        SubtitleQueryMediaKind::Movie => true,
        SubtitleQueryMediaKind::Episode => request.season.is_some() && request.episode.is_some(),
    }
}

/// The IMDb ID Scryer already holds, with no provider call involved.
fn direct_imdb_id(request: &SubtitlePluginSearchRequest) -> Option<String> {
    match request.media_kind {
        SubtitleQueryMediaKind::Episode => {
            // Wizdom keys series on the series IMDb ID, so an episode-level ID
            // from `external_ids` would look up the wrong title.
            request.series_imdb_id.clone()
        }
        SubtitleQueryMediaKind::Movie => request.imdb_id.clone().or_else(|| {
            request
                .external_ids
                .get("imdb")
                .and_then(|values| values.iter().find(|value| !value.trim().is_empty()))
                .cloned()
        }),
    }
    .and_then(|value| normalize_imdb_id(&value))
}

/// Scryer's own IMDb ID is preferred; the optional TMDB key only backfills a
/// title lookup, so this provider never ships a shared third-party credential.
fn resolve_imdb_id(
    config: &WizdomConfig,
    request: &SubtitlePluginSearchRequest,
) -> Result<Option<String>, Failure> {
    if let Some(direct) = direct_imdb_id(request) {
        return Ok(Some(direct));
    }

    let Some(api_key) = config.tmdb_api_key.as_deref() else {
        return Ok(None);
    };
    let Some(title) = title_for_search(request) else {
        return Ok(None);
    };
    lookup_imdb_id_via_tmdb(api_key, &title, request.year, request.media_kind)
}

fn lookup_imdb_id_via_tmdb(
    api_key: &str,
    title: &str,
    year: Option<i32>,
    media_kind: SubtitleQueryMediaKind,
) -> Result<Option<String>, Failure> {
    let category = match media_kind {
        SubtitleQueryMediaKind::Movie => "movie",
        SubtitleQueryMediaKind::Episode => "tv",
    };

    let mut params = vec![
        ("api_key", api_key.to_string()),
        ("query", title.to_string()),
        ("language", "en".to_string()),
    ];
    if let Some(year) = year {
        params.push(("year", year.to_string()));
    }
    let search_url = format!(
        "{TMDB_API_BASE}/search/{category}?{}",
        encode_query(&params)
    );
    let search: Value = retry_request(
        || http_get_json("Wizdom TMDB search", &search_url),
        RETRY_AMOUNT,
        RETRY_TIMEOUT_SECS,
    )?;

    let Some(tmdb_id) = search
        .get("results")
        .and_then(Value::as_array)
        .and_then(|results| results.first())
        .and_then(|result| result.get("id"))
        .and_then(Value::as_i64)
    else {
        return Ok(None);
    };

    let detail_path = match media_kind {
        SubtitleQueryMediaKind::Movie => format!("{TMDB_API_BASE}/movie/{tmdb_id}"),
        SubtitleQueryMediaKind::Episode => format!("{TMDB_API_BASE}/tv/{tmdb_id}/external_ids"),
    };
    let detail_url = format!(
        "{detail_path}?{}",
        encode_query(&[("api_key", api_key.to_string())])
    );
    let detail: Value = retry_request(
        || http_get_json("Wizdom TMDB lookup", &detail_url),
        RETRY_AMOUNT,
        RETRY_TIMEOUT_SECS,
    )?;

    Ok(detail
        .get("imdb_id")
        .and_then(Value::as_str)
        .and_then(normalize_imdb_id))
}

fn fetch_releases(config: &WizdomConfig, imdb_id: &str) -> Result<WizdomReleases, Failure> {
    let url = format!("{}/api/releases/{imdb_id}", config.base_url);
    retry_request(|| http_get_releases(&url), RETRY_AMOUNT, RETRY_TIMEOUT_SECS)
}

/// `subs` arrives in three shapes: a flat array for movies, and for series
/// either an array indexed by season number or an object keyed by the season
/// number as a string. Episodes are always keyed by the stringified number.
fn collect_subs(
    subs: Option<&Value>,
    media_kind: SubtitleQueryMediaKind,
    season: Option<i32>,
    episode: Option<i32>,
) -> Vec<WizdomSub> {
    let Some(subs) = subs else {
        return Vec::new();
    };

    match media_kind {
        SubtitleQueryMediaKind::Movie => parse_sub_array(subs),
        SubtitleQueryMediaKind::Episode => {
            let (Some(season), Some(episode)) = (season, episode) else {
                return Vec::new();
            };
            let Some(season_node) = season_node(subs, season) else {
                return Vec::new();
            };
            let Some(episode_node) = season_node.get(episode.to_string()) else {
                return Vec::new();
            };
            parse_sub_array(episode_node)
        }
    }
}

fn season_node(subs: &Value, season: i32) -> Option<&Value> {
    match subs {
        Value::Array(seasons) => usize::try_from(season)
            .ok()
            .and_then(|index| seasons.get(index)),
        Value::Object(_) => subs.get(season.to_string()),
        _ => None,
    }
}

fn parse_sub_array(node: &Value) -> Vec<WizdomSub> {
    node.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<WizdomSub>(item.clone()).ok())
                .filter(|sub| subtitle_id(sub).is_some())
                .collect()
        })
        .unwrap_or_default()
}

fn sub_to_candidate(
    config: &WizdomConfig,
    request: &SubtitlePluginSearchRequest,
    sub: &WizdomSub,
    imdb_id: &str,
) -> Option<SubtitlePluginCandidate> {
    let subtitle_id = subtitle_id(sub)?;
    let filename = format!("{subtitle_id}.zip");
    let page_url = Some(page_url(config, imdb_id, request.media_kind));
    let provider_file_id = serde_json::to_string(&WizdomDownloadRef {
        subtitle_id,
        filename,
        page_url,
    })
    .ok()?;

    Some(SubtitlePluginCandidate {
        provider_file_id,
        language: PROVIDER_LANGUAGE.to_string(),
        release_info: normalize_non_empty(&sub.version),
        hearing_impaired: false,
        forced: false,
        ai_translated: false,
        machine_translated: false,
        uploader: None,
        download_count: None,
        match_hints: build_match_hints(request, sub),
    })
}

fn build_match_hints(
    request: &SubtitlePluginSearchRequest,
    sub: &WizdomSub,
) -> Vec<SubtitleMatchHint> {
    let mut match_hints = vec![
        SubtitleMatchHint {
            kind: SubtitleMatchHintKind::Title,
            value: None,
        },
        SubtitleMatchHint {
            kind: SubtitleMatchHintKind::Language,
            value: Some(PROVIDER_LANGUAGE.to_string()),
        },
    ];

    match request.media_kind {
        SubtitleQueryMediaKind::Movie => {
            if request.imdb_id.is_some() {
                match_hints.push(SubtitleMatchHint {
                    kind: SubtitleMatchHintKind::ImdbId,
                    value: None,
                });
            }
        }
        SubtitleQueryMediaKind::Episode => {
            if request.series_imdb_id.is_some() {
                match_hints.push(SubtitleMatchHint {
                    kind: SubtitleMatchHintKind::SeriesImdbId,
                    value: None,
                });
            }
            // The season/episode keys are how the row was located, so a
            // returned row always matches the requested episode.
            match_hints.push(SubtitleMatchHint {
                kind: SubtitleMatchHintKind::SeasonEpisode,
                value: None,
            });
        }
    }

    if let Some(release) = normalize_non_empty(&sub.version) {
        match_hints.push(SubtitleMatchHint {
            kind: SubtitleMatchHintKind::Release,
            value: Some(release),
        });
    }

    match_hints
}

fn download_subtitle_impl(
    config: &WizdomConfig,
    reference: &WizdomDownloadRef,
) -> Result<SubtitlePluginDownloadResponse, Failure> {
    let url = format!(
        "{}/api/files/sub/{}",
        config.base_url, reference.subtitle_id
    );
    let artifact = retry_request(
        || http_get_download(&url, reference.page_url.as_deref()),
        RETRY_AMOUNT,
        RETRY_TIMEOUT_SECS,
    )?;

    if artifact.bytes.is_empty() {
        return Err(Failure::new(
            FailureKind::Provider,
            "Wizdom download returned an empty body",
        ));
    }

    let filename = artifact
        .filename
        .or_else(|| normalize_non_empty(&reference.filename))
        .unwrap_or_else(|| format!("{}.zip", reference.subtitle_id));
    let content_type = artifact
        .content_type
        .or_else(|| Some("application/zip".to_string()));

    // Wizdom always serves a zip; archives stay packed for Scryer's normal
    // archive handling rather than being unpacked inside the plugin.
    Ok(SubtitlePluginDownloadResponse {
        content_base64: BASE64.encode(artifact.bytes),
        format: file_extension(&filename).unwrap_or("zip").to_string(),
        filename: Some(filename),
        content_type,
    })
}

fn http_get_releases(url: &str) -> Result<WizdomReleases, Failure> {
    let request = HttpRequest::new(url)
        .with_method("GET")
        .with_header("Accept", "application/json")
        .with_header("User-Agent", USER_AGENT);
    let response = http::request::<Vec<u8>>(&request, None).map_err(|error| {
        Failure::new(
            FailureKind::Unreachable,
            format!("Wizdom releases request failed: {error}"),
        )
    })?;

    // The releases endpoint answers with HTTP 500 for an IMDb ID it does not
    // carry, which is a miss rather than a provider fault.
    if response.status_code() == 500 {
        return Ok(WizdomReleases::default());
    }

    map_http_status("Wizdom releases", &response)?;
    serde_json::from_slice(&response.body()).or_else(|error| {
        if response.body().is_empty() {
            Ok(WizdomReleases::default())
        } else {
            Err(Failure::new(
                FailureKind::Unsupported,
                format!("Wizdom JSON parse error: {error}"),
            ))
        }
    })
}

fn http_get_json(label: &str, url: &str) -> Result<Value, Failure> {
    let request = HttpRequest::new(url)
        .with_method("GET")
        .with_header("Accept", "application/json")
        .with_header("User-Agent", USER_AGENT);
    let response = http::request::<Vec<u8>>(&request, None).map_err(|error| {
        Failure::new(
            FailureKind::Unreachable,
            format!("{label} request failed: {error}"),
        )
    })?;

    map_http_status(label, &response)?;
    serde_json::from_slice(&response.body()).map_err(|error| {
        Failure::new(
            FailureKind::Unsupported,
            format!("{label} JSON parse error: {error}"),
        )
    })
}

fn http_get_download(url: &str, page_url: Option<&str>) -> Result<DownloadArtifact, Failure> {
    let mut request = HttpRequest::new(url)
        .with_method("GET")
        .with_header("Accept", "*/*")
        .with_header("User-Agent", USER_AGENT);
    if let Some(page_url) = page_url {
        request = request.with_header("Referer", page_url);
    }
    let response = http::request::<Vec<u8>>(&request, None).map_err(|error| {
        Failure::new(
            FailureKind::Unreachable,
            format!("Wizdom download request failed: {error}"),
        )
    })?;

    map_http_status("Wizdom download", &response)?;

    let bytes = response.body();
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err(Failure::new(
            FailureKind::Unsupported,
            format!(
                "Wizdom download exceeded {MAX_DOWNLOAD_BYTES} bytes ({} bytes)",
                bytes.len()
            ),
        ));
    }

    Ok(DownloadArtifact {
        content_type: response_header(&response, "content-type")
            .and_then(|value| normalize_non_empty(&value)),
        filename: response_header(&response, "content-disposition")
            .and_then(|value| content_disposition_filename(&value)),
        bytes,
    })
}

fn map_http_status(label: &str, response: &HttpResponse) -> Result<(), Failure> {
    let status = response.status_code();
    // Only decode a body preview on the error paths; a successful download body
    // is a zip that would otherwise be read twice.
    if (200..=299).contains(&status) {
        return Ok(());
    }
    map_http_status_details(
        label,
        status,
        &response_body_preview(response),
        retry_after_seconds(response),
    )
}

fn map_http_status_details(
    label: &str,
    status: u16,
    body_text: &str,
    retry_after_seconds: Option<i64>,
) -> Result<(), Failure> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(Failure::new(
            FailureKind::AuthFailed,
            format!("{label} authentication failed"),
        )),
        429 => {
            let message = match retry_after_seconds {
                Some(seconds) if seconds > 0 => {
                    format!("{label} rate limited — retry after {seconds}s")
                }
                _ => format!("{label} rate limited — try again later"),
            };
            Err(Failure::new(FailureKind::RateLimited, message)
                .with_retry_after(retry_after_seconds))
        }
        status => Err(Failure::new(
            FailureKind::Unsupported,
            format!("{label} returned HTTP {status}: {body_text}"),
        )),
    }
}

fn retry_request<T, F>(mut f: F, amount: usize, retry_timeout_secs: u64) -> Result<T, Failure>
where
    F: FnMut() -> Result<T, Failure>,
{
    let mut last_error = None;
    for attempt in 0..amount {
        match f() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retryable = matches!(
                    error.kind,
                    FailureKind::RateLimited | FailureKind::Unreachable
                );
                if !retryable || attempt + 1 >= amount {
                    return Err(error);
                }
                last_error = Some(error);
                std::thread::sleep(Duration::from_secs(retry_timeout_secs));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| Failure::new(FailureKind::Unsupported, "Wizdom request failed")))
}

fn validation_error_response(failure: &Failure) -> SubtitlePluginValidateConfigResponse {
    let status = match failure.kind {
        FailureKind::InvalidConfig => SubtitleValidateConfigStatus::InvalidConfig,
        FailureKind::AuthFailed => SubtitleValidateConfigStatus::AuthFailed,
        FailureKind::RateLimited => SubtitleValidateConfigStatus::RateLimited,
        FailureKind::Unreachable => SubtitleValidateConfigStatus::Unreachable,
        FailureKind::Unsupported | FailureKind::Provider => {
            SubtitleValidateConfigStatus::Unsupported
        }
    };
    SubtitlePluginValidateConfigResponse {
        status,
        message: Some(failure.message.clone()),
        retry_after_seconds: failure.retry_after_seconds,
    }
}

fn config_string(key: &str) -> Result<Option<String>, Failure> {
    match config::get(key) {
        Ok(value) => Ok(value.as_deref().and_then(normalize_non_empty)),
        Err(error) => Err(Failure::new(
            FailureKind::InvalidConfig,
            format!("failed to read config value '{key}': {error}"),
        )),
    }
}

fn requests_hebrew(request: &SubtitlePluginSearchRequest) -> bool {
    request
        .languages
        .iter()
        .any(|language| is_hebrew_language(language))
}

fn is_hebrew_language(code: &str) -> bool {
    let normalized = code.trim().to_ascii_lowercase();
    let base = normalized
        .split(['-', '_'])
        .next()
        .unwrap_or(normalized.as_str());
    // `iw` is the legacy ISO 639-1 code for Hebrew and still appears in the wild.
    matches!(base, "heb" | "he" | "iw")
}

fn title_for_search(request: &SubtitlePluginSearchRequest) -> Option<String> {
    request
        .title_candidates
        .iter()
        .chain(std::iter::once(&request.title))
        .chain(request.title_aliases.iter())
        .find_map(|candidate| normalize_non_empty(candidate))
}

fn normalize_imdb_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix("tt").unwrap_or(trimmed);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("tt{digits}"))
}

fn subtitle_id(sub: &WizdomSub) -> Option<String> {
    match &sub.id {
        Value::String(value) => normalize_non_empty(value),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn page_url(config: &WizdomConfig, imdb_id: &str, media_kind: SubtitleQueryMediaKind) -> String {
    let section = match media_kind {
        SubtitleQueryMediaKind::Movie => "movies",
        SubtitleQueryMediaKind::Episode => "series",
    };
    format!("{}/{section}/{imdb_id}", config.base_url)
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn file_extension(filename: &str) -> Option<&str> {
    filename.rsplit_once('.').map(|(_, extension)| extension)
}

fn response_header(response: &HttpResponse, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .or_else(|| response.headers().get(name.to_ascii_lowercase().as_str()))
        .or_else(|| response.headers().get(name.to_ascii_uppercase().as_str()))
        .cloned()
}

fn content_disposition_filename(value: &str) -> Option<String> {
    for part in value.split(';').skip(1) {
        let trimmed = part.trim();
        if let Some(raw) = trimmed.strip_prefix("filename=") {
            return normalize_non_empty(raw.trim().trim_matches('"'));
        }
    }
    None
}

fn response_body_preview(response: &HttpResponse) -> String {
    let body = response.body();
    let text = String::from_utf8_lossy(&body);
    let trimmed = text.trim();
    match trimmed.char_indices().nth(ERROR_BODY_PREVIEW_LIMIT) {
        Some((index, _)) => trimmed[..index].to_string(),
        None => trimmed.to_string(),
    }
}

fn retry_after_seconds(response: &HttpResponse) -> Option<i64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|seconds| *seconds > 0 && *seconds <= MAX_RATE_LIMIT_WAIT_SECONDS)
}

fn encode_query(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{key}={}", url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WizdomConfig {
        WizdomConfig {
            base_url: DEFAULT_BASE_URL.to_string(),
            tmdb_api_key: None,
        }
    }

    fn sub(id: i64, version: &str) -> Value {
        serde_json::json!({ "id": id, "version": version })
    }

    fn search_request(value: Value) -> SubtitlePluginSearchRequest {
        serde_json::from_value(value).expect("search request fixture")
    }

    #[test]
    fn movie_lookups_prefer_the_request_imdb_id() {
        let request = search_request(serde_json::json!({
            "media_kind": "movie",
            "title": "Inception",
            "imdb_id": "tt1375666",
            "external_ids": { "imdb": ["tt9999999"] },
        }));
        assert_eq!(direct_imdb_id(&request), Some("tt1375666".to_string()));
    }

    #[test]
    fn movie_lookups_fall_back_to_external_imdb_ids() {
        let request = search_request(serde_json::json!({
            "media_kind": "movie",
            "title": "Inception",
            "external_ids": { "imdb": ["tt1375666"] },
        }));
        assert_eq!(direct_imdb_id(&request), Some("tt1375666".to_string()));
    }

    #[test]
    fn episode_lookups_use_the_series_id_and_ignore_external_imdb_ids() {
        let request = search_request(serde_json::json!({
            "media_kind": "episode",
            "title": "Breaking Bad",
            "series_imdb_id": "tt0903747",
            "imdb_id": "tt1054724",
            "external_ids": { "imdb": ["tt1054724"] },
            "season": 1,
            "episode": 1,
        }));
        assert_eq!(direct_imdb_id(&request), Some("tt0903747".to_string()));

        // Without a series ID there is nothing safe to query, and no TMDB key
        // is configured to backfill one.
        let episode_only = search_request(serde_json::json!({
            "media_kind": "episode",
            "title": "Breaking Bad",
            "external_ids": { "imdb": ["tt1054724"] },
            "season": 1,
            "episode": 1,
        }));
        assert_eq!(direct_imdb_id(&episode_only), None);
    }

    #[test]
    fn searches_without_hebrew_return_no_candidates() {
        let request = search_request(serde_json::json!({
            "media_kind": "movie",
            "title": "Inception",
            "imdb_id": "tt1375666",
            "languages": ["eng", "fra"],
        }));
        assert!(!is_searchable(&request));
    }

    #[test]
    fn episode_searches_without_a_season_or_episode_return_no_candidates() {
        let request = search_request(serde_json::json!({
            "media_kind": "episode",
            "title": "Breaking Bad",
            "series_imdb_id": "tt0903747",
            "languages": ["heb"],
            "season": 1,
        }));
        assert!(!is_searchable(&request));
    }

    #[test]
    fn hebrew_language_codes_are_recognized() {
        for code in ["heb", "he", "HE", "iw", "he-IL", "heb_IL", " heb "] {
            assert!(is_hebrew_language(code), "expected {code} to be Hebrew");
        }
        for code in ["eng", "en", "hin", ""] {
            assert!(!is_hebrew_language(code), "expected {code} to be rejected");
        }
    }

    #[test]
    fn imdb_ids_normalize_to_the_tt_prefixed_form() {
        assert_eq!(normalize_imdb_id("tt1375666").as_deref(), Some("tt1375666"));
        assert_eq!(normalize_imdb_id("1375666").as_deref(), Some("tt1375666"));
        assert_eq!(
            normalize_imdb_id(" tt1375666 ").as_deref(),
            Some("tt1375666")
        );
        assert_eq!(normalize_imdb_id("tt"), None);
        assert_eq!(normalize_imdb_id("not-an-id"), None);
        assert_eq!(normalize_imdb_id(""), None);
    }

    #[test]
    fn movie_subs_parse_from_a_flat_array() {
        let subs = serde_json::json!([sub(11, "1080p.WEB"), sub(12, "720p.BluRay")]);
        let parsed = collect_subs(Some(&subs), SubtitleQueryMediaKind::Movie, None, None);
        assert_eq!(parsed.len(), 2);
        assert_eq!(subtitle_id(&parsed[0]).as_deref(), Some("11"));
        assert_eq!(parsed[1].version, "720p.BluRay");
    }

    #[test]
    fn episode_subs_parse_from_a_season_indexed_array() {
        // Index 0 is the placeholder slot, so season 1 sits at index 1.
        let subs = serde_json::json!([
            {},
            { "3": [sub(21, "S01E03.WEB")] },
        ]);
        let parsed = collect_subs(
            Some(&subs),
            SubtitleQueryMediaKind::Episode,
            Some(1),
            Some(3),
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(subtitle_id(&parsed[0]).as_deref(), Some("21"));
    }

    #[test]
    fn episode_subs_parse_from_a_season_keyed_object() {
        let subs = serde_json::json!({
            "2": { "5": [sub(31, "S02E05.HDTV"), sub(32, "S02E05.WEB")] },
        });
        let parsed = collect_subs(
            Some(&subs),
            SubtitleQueryMediaKind::Episode,
            Some(2),
            Some(5),
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].version, "S02E05.HDTV");
    }

    #[test]
    fn missing_season_or_episode_keys_yield_no_subs() {
        let subs = serde_json::json!({ "2": { "5": [sub(31, "S02E05.HDTV")] } });
        for (season, episode) in [(Some(3), Some(5)), (Some(2), Some(9))] {
            let parsed = collect_subs(
                Some(&subs),
                SubtitleQueryMediaKind::Episode,
                season,
                episode,
            );
            assert!(parsed.is_empty());
        }
        assert!(collect_subs(None, SubtitleQueryMediaKind::Movie, None, None).is_empty());
    }

    #[test]
    fn rows_without_a_usable_id_are_dropped() {
        let subs = serde_json::json!([
            sub(11, "kept"),
            { "version": "no id" },
            { "id": null, "version": "null id" },
        ]);
        let parsed = collect_subs(Some(&subs), SubtitleQueryMediaKind::Movie, None, None);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].version, "kept");
    }

    #[test]
    fn download_refs_round_trip_through_the_provider_file_id() {
        let reference = WizdomDownloadRef {
            subtitle_id: "42".to_string(),
            filename: "42.zip".to_string(),
            page_url: Some("https://wizdom.xyz/movies/tt1375666".to_string()),
        };
        let encoded = serde_json::to_string(&reference).expect("serialize");
        let decoded: WizdomDownloadRef = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, reference);
    }

    #[test]
    fn page_urls_use_the_media_specific_section() {
        let config = config();
        assert_eq!(
            page_url(&config, "tt1375666", SubtitleQueryMediaKind::Movie),
            "https://wizdom.xyz/movies/tt1375666"
        );
        assert_eq!(
            page_url(&config, "tt0903747", SubtitleQueryMediaKind::Episode),
            "https://wizdom.xyz/series/tt0903747"
        );
    }

    #[test]
    fn http_status_mapping_matches_provider_semantics() {
        assert!(map_http_status_details("probe", 200, "", None).is_ok());
        assert_eq!(
            map_http_status_details("probe", 403, "", None)
                .unwrap_err()
                .kind,
            FailureKind::AuthFailed
        );
        let rate_limited = map_http_status_details("probe", 429, "", Some(5)).unwrap_err();
        assert_eq!(rate_limited.kind, FailureKind::RateLimited);
        assert_eq!(rate_limited.retry_after_seconds, Some(5));
        assert_eq!(
            map_http_status_details("probe", 404, "missing", None)
                .unwrap_err()
                .kind,
            FailureKind::Unsupported
        );
    }

    #[test]
    fn content_disposition_filenames_are_extracted() {
        assert_eq!(
            content_disposition_filename("attachment; filename=\"wizdom.42.zip\"").as_deref(),
            Some("wizdom.42.zip")
        );
        assert_eq!(content_disposition_filename("attachment"), None);
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            encode_query(&[("query", "The Dark Knight".to_string())]),
            "query=The%20Dark%20Knight"
        );
    }
}
