//! Simkl list provider.
//!
//! Simkl keeps one library per member, split into shows, anime and movies and
//! sorted by status: watching, plan to watch, on hold, completed and dropped
//! (movies are never watching or on hold). Each source here is one of those
//! statuses, read with the member's own Simkl token from
//! `/sync/all-items/{type}/{status}`. Nothing is public, so a fetch without the
//! member's credential is refused before any request.
//!
//! Simkl asks every app that syncs on a timer to read `/sync/activities` first
//! and to skip the library read when nothing moved. The fetch does that: the
//! fingerprint it returns is made of the activity timestamps of the libraries
//! it reads, and when they match the fingerprint the host already holds the
//! library is not read at all. A library always answers in one response, so a
//! fingerprint taken before the read can only cause an extra read later,
//! never a missed one.
//!
//! Anime seasons are separate entries on Simkl. Each stays its own item, keyed
//! by its Simkl id, and when Simkl maps the entry onto exactly one TVDB season
//! the item carries that season, so the host can attach it to the parent TVDB
//! series.
//!
//! Every request names Scryer's Simkl app by its client id. The member's
//! token travels only in the `Authorization` header and never appears in a
//! URL or an error message.

use std::collections::BTreeSet;

use list_provider_common::error::{
    Access, auth_failed, check_status, invalid_config, permanent, plugin_error, unavailable,
    unsupported_source,
};
use list_provider_common::http::{HostHttp, ListHttp, encode_component, get, json_body};
use list_provider_common::ids::{
    Ids, dedupe_and_rank, external_ids, item_key, json_id, json_text, json_year, kind_str,
};
use list_provider_common::page::single_page;
use scryer_plugin_sdk::command::{PluginListCommand, PluginListCommandResult};
use scryer_plugin_sdk::host::{PluginHttpRequest, PluginHttpResponse};
use scryer_plugin_sdk::{
    ListAccountExchange, ListAccountFlow, ListAccountStatus, ListAuthBadge, ListCredential,
    ListExternalId, ListMediaKind, ListNoteTone, ListPluginAccountResponse, ListPluginFetchRequest,
    ListPluginFetchResponse, ListPluginItem, ListProviderAuth, ListProviderCapabilities,
    ListProviderDescriptor, ListProviderGroup, ListProviderItem, ListProviderNote,
    ListProviderTile, ListSourceParam, ListSourceParamType, PluginDescriptor, PluginError,
    PluginErrorCode, PluginResult, ProviderDescriptor,
};
use serde_json::Value;

wit_bindgen::generate!({
    world: "scryer:lists/list-provider@1.0.0",
    path: ["wit/host-v1.0.0", "wit/runtime-v1.0.0", "wit/list-v1.0.0"],
    generate_all,
});

list_provider_common::list_component_main!(descriptor = descriptor, handler = handle_command,);

pub const PLUGIN_ID: &str = "simkl-list";
pub const PROVIDER_TYPE: &str = "simkl";

/// The client id of Scryer's own Simkl app, sent with every request. It stays
/// empty until that app is registered with Simkl, and every command refuses to
/// run while it is.
pub const SIMKL_CLIENT_ID: &str = "";

pub const SOURCE_WATCHING: &str = "watching";
pub const SOURCE_PLAN_TO_WATCH: &str = "plantowatch";
pub const SOURCE_ON_HOLD: &str = "hold";
pub const SOURCE_COMPLETED: &str = "completed";
pub const SOURCE_DROPPED: &str = "dropped";

/// Narrows a status to one of Simkl's libraries; absent or `all` reads every
/// library the status exists in.
pub const PARAM_TYPE: &str = "type";
pub const TYPE_ALL: &str = "all";
pub const TYPE_MOVIES: &str = "movies";
pub const TYPE_SHOWS: &str = "shows";
pub const TYPE_ANIME: &str = "anime";

pub const API_BASE: &str = "https://api.simkl.com";
const API_HOST: &str = "api.simkl.com";
/// The app name and version Simkl asks every request to carry.
const APP_NAME: &str = "scryer";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const ACCEPT: &str = "application/json";
/// Adds the TVDB season mapping to anime entries.
const EXTENDED_ANIME_SEASONS: &str = "full_anime_seasons";
/// Six hours, the interval the other arrs use for Simkl.
const DEFAULT_INTERVAL_SECONDS: u64 = 6 * 60 * 60;
/// The longest Simkl error name repeated in an error message.
const MAX_ERROR_NAME_LEN: usize = 40;
/// The host's external-id kind for ids that name an anime entry.
const ANIME_ID_KIND: &str = "anime";

/// A Simkl status, which is also the source type that follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Watching,
    PlanToWatch,
    OnHold,
    Completed,
    Dropped,
}

impl Status {
    pub const ALL: [Status; 5] = [
        Status::Watching,
        Status::PlanToWatch,
        Status::OnHold,
        Status::Completed,
        Status::Dropped,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Watching => SOURCE_WATCHING,
            Self::PlanToWatch => SOURCE_PLAN_TO_WATCH,
            Self::OnHold => SOURCE_ON_HOLD,
            Self::Completed => SOURCE_COMPLETED,
            Self::Dropped => SOURCE_DROPPED,
        }
    }

    pub fn parse(source_type: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.key() == source_type)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Watching => "Watching",
            Self::PlanToWatch => "Plan to watch",
            Self::OnHold => "On hold",
            Self::Completed => "Completed",
            Self::Dropped => "Dropped",
        }
    }

    fn item_id(self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::PlanToWatch => "plan-to-watch",
            Self::OnHold => "on-hold",
            Self::Completed => "completed",
            Self::Dropped => "dropped",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Watching => "Shows and anime marked as watching",
            Self::PlanToWatch => "Movies, shows and anime marked as plan to watch",
            Self::OnHold => "Shows and anime put on hold",
            Self::Completed => "Movies, shows and anime marked as completed",
            Self::Dropped => "Movies, shows and anime marked as dropped",
        }
    }

    /// Simkl has no watching or on-hold status for movies.
    fn has_movies(self) -> bool {
        !matches!(self, Self::Watching | Self::OnHold)
    }

    fn kinds(self) -> Vec<ListMediaKind> {
        let mut kinds = Vec::with_capacity(3);
        if self.has_movies() {
            kinds.push(ListMediaKind::Movie);
        }
        kinds.push(ListMediaKind::Series);
        kinds.push(ListMediaKind::Anime);
        kinds
    }

    fn type_options(self) -> Vec<String> {
        let mut options = vec![TYPE_ALL];
        if self.has_movies() {
            options.push(TYPE_MOVIES);
        }
        options.extend([TYPE_SHOWS, TYPE_ANIME]);
        options.into_iter().map(str::to_string).collect()
    }
}

/// One of the three libraries a Simkl member's items live in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Library {
    Shows,
    Anime,
    Movies,
}

impl Library {
    /// The path segment of `/sync/all-items` and the key of its response.
    fn path(self) -> &'static str {
        match self {
            Self::Shows => TYPE_SHOWS,
            Self::Anime => TYPE_ANIME,
            Self::Movies => TYPE_MOVIES,
        }
    }

    /// The library's block in `/sync/activities`.
    fn activity_key(self) -> &'static str {
        match self {
            Self::Shows => "tv_shows",
            Self::Anime => "anime",
            Self::Movies => "movies",
        }
    }

    /// The segment that scopes a Simkl id in an item key.
    fn key_scope(self) -> &'static str {
        match self {
            Self::Shows => "show",
            Self::Anime => "anime",
            Self::Movies => "movie",
        }
    }

    fn extended(self) -> Option<&'static str> {
        (self == Self::Anime).then_some(EXTENDED_ANIME_SEASONS)
    }
}

/// The libraries a status source reads, in output order.
fn libraries(
    status: Status,
    request: &ListPluginFetchRequest,
) -> Result<Vec<Library>, PluginError> {
    let requested = request
        .params
        .get(PARAM_TYPE)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match requested.as_deref() {
        None | Some(TYPE_ALL) => {
            let mut libraries = vec![Library::Shows, Library::Anime];
            if status.has_movies() {
                libraries.push(Library::Movies);
            }
            Ok(libraries)
        }
        Some(TYPE_SHOWS) => Ok(vec![Library::Shows]),
        Some(TYPE_ANIME) => Ok(vec![Library::Anime]),
        Some(TYPE_MOVIES) if status.has_movies() => Ok(vec![Library::Movies]),
        Some(TYPE_MOVIES) => Err(invalid_config(format!(
            "Simkl has no {} list for movies",
            status.label().to_ascii_lowercase()
        ))),
        Some(other) => Err(invalid_config(format!(
            "unknown Simkl type {other}; use all, movies, shows or anime"
        ))),
    }
}

fn status_item(status: Status) -> ListProviderItem {
    ListProviderItem {
        id: status.item_id().to_string(),
        name: status.label().to_string(),
        description: Some(status.description().to_string()),
        kinds: status.kinds(),
        source_type: status.key().to_string(),
        params: vec![ListSourceParam {
            key: PARAM_TYPE.to_string(),
            label: "Type".to_string(),
            param_type: ListSourceParamType::Enum,
            options: status.type_options(),
            required: false,
        }],
        personal: true,
        default_interval_seconds: DEFAULT_INTERVAL_SECONDS,
    }
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID.to_string(),
        name: "Simkl".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: scryer_plugin_sdk::SDK_VERSION.to_string(),
        sdk_constraint: scryer_plugin_sdk::current_sdk_constraint(),
        socket_permissions: Vec::new(),
        provider: ProviderDescriptor::ListProvider(ListProviderDescriptor {
            provider_type: PROVIDER_TYPE.to_string(),
            provider_aliases: Vec::new(),
            summary: Some("A Simkl member's movies, shows and anime by status".to_string()),
            blurb: Some(
                "Link a Simkl account to follow what it is watching, plans to watch, has on \
                 hold, completed or dropped. Titles are matched by the ids Simkl keeps for \
                 them, and each anime season is its own entry."
                    .to_string(),
            ),
            tile: Some(ListProviderTile {
                bg: "#000000".to_string(),
                ink: "#ffffff".to_string(),
                abbr: "SK".to_string(),
            }),
            brand_url_template: None,
            coverage: vec![
                ListMediaKind::Movie,
                ListMediaKind::Series,
                ListMediaKind::Anime,
            ],
            auth: ListProviderAuth::MemberAccount {
                flow: ListAccountFlow::Pin,
                exchange: ListAccountExchange::Direct,
                byo_app: false,
                scopes: Vec::new(),
            },
            groups: vec![ListProviderGroup {
                label: "Your Simkl library".to_string(),
                auth_badge: ListAuthBadge::MemberAccount,
                items: Status::ALL.into_iter().map(status_item).collect(),
            }],
            notes: vec![
                ListProviderNote {
                    tone: ListNoteTone::Info,
                    text_key: "lists.note.simkl_anime_seasons".to_string(),
                },
                ListProviderNote {
                    tone: ListNoteTone::Info,
                    text_key: "lists.note.simkl_pin".to_string(),
                },
            ],
            url_patterns: Vec::new(),
            capabilities: ListProviderCapabilities {
                account: true,
                health: false,
                requires_member_credential: true,
            },
            config_fields: Vec::new(),
            default_base_url: None,
            allowed_hosts: vec![API_HOST.to_string()],
            rate_limit_seconds: Some(1),
        }),
    }
}

async fn handle_command(command: PluginListCommand) -> PluginListCommandResult {
    run(&HostHttp, SIMKL_CLIENT_ID, command).await
}

pub async fn run<H: ListHttp>(
    http: &H,
    client_id: &str,
    command: PluginListCommand,
) -> PluginListCommandResult {
    match command {
        PluginListCommand::Fetch(request) => {
            PluginListCommandResult::Fetch(into_result(fetch(http, client_id, &request).await))
        }
        PluginListCommand::Account(request) => PluginListCommandResult::Account(into_result(
            account(http, client_id, &request.credential).await,
        )),
        PluginListCommand::Health(_) => {
            PluginListCommandResult::Health(PluginResult::Err(plugin_error(
                PluginErrorCode::Unsupported,
                "Simkl lists use each member's own account; there is no server key to check",
            )))
        }
    }
}

fn into_result<T>(result: Result<T, PluginError>) -> PluginResult<T> {
    match result {
        Ok(value) => PluginResult::Ok(value),
        Err(error) => PluginResult::Err(error),
    }
}

pub async fn fetch<H: ListHttp>(
    http: &H,
    client_id: &str,
    request: &ListPluginFetchRequest,
) -> Result<ListPluginFetchResponse, PluginError> {
    let status = Status::parse(&request.source_type)
        .ok_or_else(|| unsupported_source(&request.source_type))?;
    let libraries = libraries(status, request)?;
    let client = Client::new(http, client_id, request.credential.as_ref())?;
    client.fetch(status, &libraries, request).await
}

pub async fn account<H: ListHttp>(
    http: &H,
    client_id: &str,
    credential: &ListCredential,
) -> Result<ListPluginAccountResponse, PluginError> {
    Client::new(http, client_id, Some(credential))?
        .account()
        .await
}

struct Client<'a, H> {
    http: &'a H,
    client_id: &'a str,
    token: &'a str,
}

impl<'a, H: ListHttp> Client<'a, H> {
    fn new(
        http: &'a H,
        client_id: &'a str,
        credential: Option<&'a ListCredential>,
    ) -> Result<Self, PluginError> {
        let client_id = client_id.trim();
        if client_id.is_empty() {
            return Err(invalid_config(
                "this build of the Simkl plugin has no Simkl app id yet, so it cannot reach Simkl",
            ));
        }
        let token = credential
            .map(|credential| credential.access_token.trim())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| auth_failed("a Simkl list needs the member's linked Simkl account"))?;
        Ok(Self {
            http,
            client_id,
            token,
        })
    }

    fn url(&self, path: &str, extended: Option<&str>) -> String {
        let mut url = format!(
            "{API_BASE}{path}?client_id={}&app-name={APP_NAME}&app-version={}",
            encode_component(self.client_id),
            encode_component(APP_VERSION),
        );
        if let Some(extended) = extended {
            url.push_str("&extended=");
            url.push_str(extended);
        }
        url
    }

    fn request(&self, path: &str, extended: Option<&str>) -> PluginHttpRequest {
        let mut request = get(self.url(path, extended), USER_AGENT, ACCEPT);
        request
            .headers
            .insert("simkl-api-key".to_string(), self.client_id.to_string());
        request.headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", self.token),
        );
        request
    }

    async fn get_json(
        &self,
        path: &str,
        extended: Option<&str>,
        what: &str,
    ) -> Result<Value, PluginError> {
        let response = self.http.send(self.request(path, extended)).await?;
        check_simkl_status(&response, what)?;
        if response.body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        json_body(&response)
    }

    async fn fetch(
        &self,
        status: Status,
        libraries: &[Library],
        request: &ListPluginFetchRequest,
    ) -> Result<ListPluginFetchResponse, PluginError> {
        let list_name = Some(status.label().to_string());
        let activities = self
            .get_json("/sync/activities", None, "Simkl activity")
            .await?;
        let fingerprint = activity_fingerprint(&activities, status, libraries);
        if let Some(fingerprint) = &fingerprint
            && request.since_fingerprint.as_deref() == Some(fingerprint.as_str())
        {
            return Ok(ListPluginFetchResponse {
                list_name,
                fingerprint: Some(fingerprint.clone()),
                unchanged: true,
                ..ListPluginFetchResponse::default()
            });
        }

        let mut items = Vec::new();
        for library in libraries {
            let path = format!("/sync/all-items/{}/{}", library.path(), status.key());
            let what = format!("Simkl {} {} list", library.path(), status.key());
            let body = self.get_json(&path, library.extended(), &what).await?;
            items.extend(library_items(&body, *library)?);
        }
        let items = dedupe_and_rank(items, 1);

        Ok(match fingerprint {
            Some(fingerprint) => ListPluginFetchResponse {
                total_hint: Some(items.len() as u32),
                items,
                next_cursor: None,
                list_name,
                list_url: None,
                fingerprint: Some(fingerprint),
                unchanged: false,
            },
            // Without usable activity timestamps the contents themselves are
            // the only safe fingerprint.
            None => single_page(items, list_name, None, request.since_fingerprint.as_deref()),
        })
    }

    async fn account(&self) -> Result<ListPluginAccountResponse, PluginError> {
        let body = self
            .get_json("/users/settings", None, "Simkl account")
            .await?;
        let external_user_id =
            json_id(body.get("account").and_then(|account| account.get("id")))
                .ok_or_else(|| permanent("Simkl did not return the member's account id"))?;
        let user = body.get("user");
        let name = json_text(user.and_then(|user| user.get("name")));
        let avatar_url = json_text(user.and_then(|user| user.get("avatar")))
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"));
        Ok(ListPluginAccountResponse {
            username: name.clone().unwrap_or_else(|| external_user_id.clone()),
            external_user_id,
            display_name: name,
            avatar_url,
            owned_lists: Vec::new(),
            statuses: Status::ALL
                .into_iter()
                .map(|status| ListAccountStatus {
                    key: status.key().to_string(),
                    label: status.label().to_string(),
                    kinds: status.kinds(),
                })
                .collect(),
        })
    }
}

/// Simkl's error name from an error body's `error` field, kept only when it
/// is a plain lowercase identifier so nothing else from the body reaches a
/// message.
fn error_name(body: &Value) -> Option<&str> {
    let name = body.get("error")?.as_str()?.trim();
    let plain = !name.is_empty()
        && name.len() <= MAX_ERROR_NAME_LEN
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
    plain.then_some(name)
}

fn response_error_name(response: &PluginHttpResponse) -> Option<String> {
    let body: Value = serde_json::from_slice(&response.body).ok()?;
    error_name(&body).map(str::to_string)
}

/// Map a Simkl failure onto the host's classes. Every Simkl call here carries
/// the member's token, so a 401 or 403 means the linked account no longer
/// works; a 412 is Simkl refusing Scryer's app, not the member.
fn check_simkl_status(response: &PluginHttpResponse, what: &str) -> Result<(), PluginError> {
    let status = response.status;
    let detail = || match response_error_name(response) {
        Some(name) => format!("HTTP {status}, {name}"),
        None => format!("HTTP {status}"),
    };
    match status {
        401 | 403 => Err(auth_failed(format!(
            "Simkl rejected the linked account ({})",
            detail()
        ))),
        412 => Err(unavailable(format!(
            "Simkl refused Scryer's Simkl app ({}); it may be throttling the app",
            detail()
        ))),
        _ => check_status(response, Access::ServerKey, what),
    }
}

/// The activity timestamps of every library a source reads, or `None` when
/// any of them is missing or malformed. A timestamp Simkl reports as null
/// (no activity yet) is a real value: the first activity changes it.
fn activity_fingerprint(
    activities: &Value,
    status: Status,
    libraries: &[Library],
) -> Option<String> {
    let mut parts = Vec::with_capacity(libraries.len());
    for library in libraries {
        let stamp = match activities.get(library.activity_key())?.get("all")? {
            Value::String(stamp) if !stamp.trim().is_empty() => stamp.trim().to_string(),
            Value::Null => "-".to_string(),
            _ => return None,
        };
        parts.push(format!("{}={stamp}", library.path()));
    }
    Some(format!(
        "simkl:v1:{APP_VERSION}:{}:{}",
        status.key(),
        parts.join(",")
    ))
}

/// The items of one library response. Simkl leaves a library's key out when
/// it is empty and answers `{}` for an empty status.
fn library_items(body: &Value, library: Library) -> Result<Vec<ListPluginItem>, PluginError> {
    let entries = match body {
        Value::Null => return Ok(Vec::new()),
        Value::Array(entries) if entries.is_empty() => return Ok(Vec::new()),
        Value::Object(map) => match map.get(library.path()) {
            None | Some(Value::Null) => {
                if map.contains_key("error") {
                    let name = error_name(body).unwrap_or("unnamed");
                    return Err(permanent(format!("Simkl answered with an error ({name})")));
                }
                return Ok(Vec::new());
            }
            Some(Value::Array(entries)) => entries,
            Some(_) => {
                return Err(permanent(
                    "the Simkl library response has an unexpected shape",
                ));
            }
        },
        _ => {
            return Err(permanent(
                "the Simkl library response has an unexpected shape",
            ));
        }
    };
    Ok(entries
        .iter()
        .filter_map(|entry| entry_item(entry, library))
        .collect())
}

/// What an entry is: its kind hint, and the kind its TMDb, IMDb and TVDB ids
/// describe. Anime movies are movies; every other anime entry is anime whose
/// ids name a TV series. Music videos are not something Scryer manages.
fn entry_kinds(entry: &Value, library: Library) -> Option<(ListMediaKind, ListMediaKind)> {
    match library {
        Library::Movies => Some((ListMediaKind::Movie, ListMediaKind::Movie)),
        Library::Shows => Some((ListMediaKind::Series, ListMediaKind::Series)),
        Library::Anime => match anime_type(entry).as_deref() {
            Some("movie") => Some((ListMediaKind::Movie, ListMediaKind::Movie)),
            Some("music video") => None,
            _ => Some((ListMediaKind::Anime, ListMediaKind::Series)),
        },
    }
}

fn anime_type(entry: &Value) -> Option<String> {
    json_text(entry.get("anime_type")).map(|value| value.to_ascii_lowercase())
}

/// The one TVDB season an anime entry maps onto. An entry spread across
/// several seasons, or with no mapping, belongs to the whole series.
fn mapped_season(entry: &Value) -> Option<i32> {
    let seasons = entry.get("mapped_tvdb_seasons")?.as_array()?;
    let mut distinct = BTreeSet::new();
    for season in seasons {
        let season = i32::try_from(season.as_i64()?).ok()?;
        if season < 0 {
            return None;
        }
        distinct.insert(season);
    }
    match distinct.len() {
        1 => distinct.into_iter().next(),
        _ => None,
    }
}

fn entry_item(entry: &Value, library: Library) -> Option<ListPluginItem> {
    let media = match library {
        Library::Movies => entry.get("movie").or_else(|| entry.get("show")),
        Library::Shows | Library::Anime => entry.get("show").or_else(|| entry.get("movie")),
    }?;
    let (kind, id_kind) = entry_kinds(entry, library)?;
    let ids = media.get("ids");
    let id = |source: &str| ids.and_then(|ids| ids.get(source));

    let common = Ids::default()
        .with_tmdb(json_id(id("tmdb")))
        .with_imdb(json_text(id("imdb")))
        .with_tvdb(json_id(id("tvdb")));
    let simkl = json_id(id("simkl"));
    let title = json_text(media.get("title"));
    let year = json_year(media.get("year"));

    // Simkl's own id is stable even when Simkl later learns an entry's other
    // ids, and it keeps every anime season apart.
    let key = match &simkl {
        Some(simkl) => format!("simkl:{}:{simkl}", library.key_scope()),
        None => item_key(&common, Some(id_kind), title.as_deref(), year)?,
    };

    let mut external_ids = external_ids(&common, Some(id_kind));
    // Simkl's own id names an anime entry in the host's vocabulary when it
    // comes from the anime library, and the item's kind otherwise. MAL,
    // AniList, AniDB and Kitsu ids always name anime.
    let simkl_kind = match library {
        Library::Anime => Some(ANIME_ID_KIND),
        _ => kind_str(kind),
    };
    let mut push = |source: &str, value: Option<String>, kind: Option<&str>| {
        if let Some(value) = value {
            external_ids.push(ListExternalId {
                source: source.to_string(),
                kind: kind.map(str::to_string),
                id: value,
            });
        }
    };
    push("simkl", simkl, simkl_kind);
    for source in ["mal", "anilist", "anidb", "kitsu"] {
        push(source, json_id(id(source)), Some(ANIME_ID_KIND));
    }

    let (season, format) = match library {
        Library::Anime => (
            (kind == ListMediaKind::Anime)
                .then(|| mapped_season(entry))
                .flatten(),
            anime_type(entry),
        ),
        _ => (None, None),
    };

    Some(ListPluginItem {
        item_key: key,
        kind_hint: Some(kind),
        title,
        year,
        external_ids,
        season,
        format,
        ..ListPluginItem::default()
    })
}

#[cfg(test)]
mod tests;
