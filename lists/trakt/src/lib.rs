//! Trakt list provider.
//!
//! Follows four kinds of Trakt sources:
//!
//! - `user_list`: any public list a Trakt member made, by username and list
//!   slug or id, from `/users/{user}/lists/{list}`.
//! - `list`: any public list by its numeric id, from `/lists/{id}`, which
//!   also serves Trakt's official lists.
//! - `watchlist`: the connected member's watchlist.
//! - `my_list`: one of the connected member's own lists, private ones too.
//!
//! Public sources are always read anonymously, so they keep working when a
//! member's account link lapses. Personal sources send the member's bearer
//! token, which the host renews; the plugin never refreshes it.
//!
//! Every request carries the Scryer Trakt app's client id. Parameterless
//! charts (trending, popular, anticipated, box office and the rest) are served
//! by the metadata gateway and are deliberately absent here.

use std::collections::BTreeMap;

use list_provider_common::error::{
    Access, auth_failed, check_status, invalid_config, missing_param, not_found, permanent,
    plugin_error, unsupported_source,
};
use list_provider_common::http::{HostHttp, ListHttp, encode_component, get, header, json_body};
use list_provider_common::ids::{
    Ids, build_item, dedupe_and_rank, json_id, json_text, json_year, kind_str, positive_id,
};
use list_provider_common::page::{numeric_cursor, single_page};
use scryer_plugin_sdk::command::{PluginListCommand, PluginListCommandResult};
use scryer_plugin_sdk::host::{PluginHttpRequest, PluginHttpResponse};
use scryer_plugin_sdk::{
    ListAccountExchange, ListAccountFlow, ListAccountList, ListAuthBadge, ListCredential,
    ListExternalId, ListMediaKind, ListPluginAccountRequest, ListPluginAccountResponse,
    ListPluginFetchRequest, ListPluginFetchResponse, ListPluginItem, ListProviderAuth,
    ListProviderCapabilities, ListProviderDescriptor, ListProviderGroup, ListProviderItem,
    ListProviderTile, ListSourceParam, ListSourceParamType, ListUrlPattern, ListUrlPatternCapture,
    PluginDescriptor, PluginError, PluginErrorCode, PluginResult, ProviderDescriptor,
};
use serde_json::Value;

wit_bindgen::generate!({
    world: "scryer:lists/list-provider@1.0.0",
    path: ["wit/host-v1.0.0", "wit/runtime-v1.0.0", "wit/list-v1.0.0"],
    generate_all,
});

list_provider_common::list_component_main!(descriptor = descriptor, handler = handle_command,);

pub const PLUGIN_ID: &str = "trakt-list";
pub const PROVIDER_TYPE: &str = "trakt";

/// The Scryer Trakt app's client id, sent as `trakt-api-key` on every
/// request. It stays empty until the scryer-media app is registered with
/// Trakt; until then every fetch and account call fails with a plain
/// configuration error instead of reaching Trakt.
pub const TRAKT_CLIENT_ID: &str = "";

pub const SOURCE_USER_LIST: &str = "user_list";
pub const SOURCE_LIST: &str = "list";
pub const SOURCE_WATCHLIST: &str = "watchlist";
pub const SOURCE_MY_LIST: &str = "my_list";

pub const PARAM_USER: &str = "user";
pub const PARAM_LIST: &str = "list";
pub const PARAM_LIST_ID: &str = "list_id";

/// The external id source the metadata gateway resolves Trakt ids under.
pub const SOURCE_TRAKT: &str = "trakt";

pub const API_BASE: &str = "https://api.trakt.tv";
const API_HOST: &str = "api.trakt.tv";
const SITE_BASE: &str = "https://trakt.tv";
const API_VERSION: &str = "2";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const JSON: &str = "application/json";
/// Items asked for per page. Trakt clamps a larger limit to the endpoint's
/// maximum, so ranks follow the limit it reports back.
pub const PAGE_LIMIT: u32 = 250;
/// Deepest page followed: 10,000 entries, well inside the host's
/// hundred-page ceiling per sync.
pub const MAX_PAGES: u32 = 40;
/// Lists asked for per page when listing the member's own lists.
const ACCOUNT_LIST_LIMIT: u32 = 100;
const MAX_ACCOUNT_LIST_PAGES: u32 = 10;
/// Twelve hours, the interval the other arrs use for Trakt lists.
const DEFAULT_INTERVAL_SECONDS: u64 = 12 * 60 * 60;
/// One fetch every two seconds stays far below Trakt's 500 reads per five
/// minutes even when a page also reads the list summary.
const RATE_LIMIT_SECONDS: i64 = 2;

const NO_CLIENT_ID: &str = "this build of the Trakt plugin has no Trakt app client id, so it \
                            cannot reach Trakt yet";

fn text_param(key: &str, label: &str) -> ListSourceParam {
    ListSourceParam {
        key: key.to_string(),
        label: label.to_string(),
        param_type: ListSourceParamType::Text,
        options: Vec::new(),
        required: true,
    }
}

fn source_item(
    id: &str,
    name: &str,
    description: &str,
    source_type: &str,
    params: Vec<ListSourceParam>,
    personal: bool,
) -> ListProviderItem {
    ListProviderItem {
        id: id.to_string(),
        name: name.to_string(),
        description: Some(description.to_string()),
        kinds: vec![ListMediaKind::Movie, ListMediaKind::Series],
        source_type: source_type.to_string(),
        params,
        personal,
        default_interval_seconds: DEFAULT_INTERVAL_SECONDS,
    }
}

fn capture(group: &str) -> ListUrlPatternCapture {
    ListUrlPatternCapture {
        group: group.to_string(),
        param: group.to_string(),
    }
}

pub fn descriptor() -> PluginDescriptor {
    // The current site lives on `app.trakt.tv`; older links omit it or use
    // `www`.
    let site = r"^https?://(?:www\.|app\.)?trakt\.tv";
    PluginDescriptor {
        id: PLUGIN_ID.to_string(),
        name: "Trakt".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: scryer_plugin_sdk::SDK_VERSION.to_string(),
        sdk_constraint: scryer_plugin_sdk::current_sdk_constraint(),
        socket_permissions: Vec::new(),
        provider: ProviderDescriptor::ListProvider(ListProviderDescriptor {
            provider_type: PROVIDER_TYPE.to_string(),
            provider_aliases: Vec::new(),
            summary: Some("Public Trakt lists, your watchlist and your own lists".to_string()),
            blurb: Some(
                "Follow any public Trakt list by its address, or connect a Trakt account to \
                 follow your watchlist and your own lists. Charts such as trending and popular \
                 come from Scryer's metadata service instead."
                    .to_string(),
            ),
            tile: Some(ListProviderTile {
                bg: "#9f42c6".to_string(),
                ink: "#ffffff".to_string(),
                abbr: "TR".to_string(),
            }),
            brand_url_template: None,
            coverage: vec![ListMediaKind::Movie, ListMediaKind::Series],
            // Trakt's authorize step takes no PKCE challenge and no scopes,
            // and its token exchange needs the app secret, which only the
            // relay or an operator's own app holds.
            auth: ListProviderAuth::MemberAccount {
                flow: ListAccountFlow::AuthorizationCode { pkce: false },
                exchange: ListAccountExchange::SmgRelay,
                byo_app: true,
                scopes: Vec::new(),
            },
            groups: vec![
                ListProviderGroup {
                    label: "Public lists".to_string(),
                    auth_badge: ListAuthBadge::NoAccountNeedsValue,
                    items: vec![
                        source_item(
                            "user-list",
                            "User list",
                            "A public list a Trakt member made, by its address",
                            SOURCE_USER_LIST,
                            vec![
                                text_param(PARAM_USER, "Username"),
                                text_param(PARAM_LIST, "List slug or id"),
                            ],
                            false,
                        ),
                        source_item(
                            "public-list",
                            "List by id",
                            "A public or official Trakt list, by its numeric id",
                            SOURCE_LIST,
                            vec![text_param(PARAM_LIST_ID, "List id")],
                            false,
                        ),
                    ],
                },
                ListProviderGroup {
                    label: "Your Trakt account".to_string(),
                    auth_badge: ListAuthBadge::MemberAccount,
                    items: vec![
                        source_item(
                            "watchlist",
                            "Watchlist",
                            "Movies and shows on your Trakt watchlist",
                            SOURCE_WATCHLIST,
                            Vec::new(),
                            true,
                        ),
                        source_item(
                            "my-list",
                            "Your list",
                            "One of your own Trakt lists, private ones included",
                            SOURCE_MY_LIST,
                            vec![text_param(PARAM_LIST, "List")],
                            true,
                        ),
                    ],
                },
            ],
            notes: Vec::new(),
            url_patterns: vec![
                ListUrlPattern {
                    pattern: format!(
                        r"{site}/users/(?<{PARAM_USER}>[^/?#]+)/lists/(?<{PARAM_LIST}>[^/?#]+)/?(?:[?#].*)?$"
                    ),
                    source_type: SOURCE_USER_LIST.to_string(),
                    captures: vec![capture(PARAM_USER), capture(PARAM_LIST)],
                },
                ListUrlPattern {
                    pattern: format!(r"{site}/lists/(?<{PARAM_LIST_ID}>\d+)(?:[-/?#].*)?$"),
                    source_type: SOURCE_LIST.to_string(),
                    captures: vec![capture(PARAM_LIST_ID)],
                },
            ],
            capabilities: ListProviderCapabilities {
                account: true,
                health: false,
                requires_member_credential: false,
            },
            config_fields: Vec::new(),
            default_base_url: None,
            allowed_hosts: vec![API_HOST.to_string()],
            rate_limit_seconds: Some(RATE_LIMIT_SECONDS),
        }),
    }
}

async fn handle_command(command: PluginListCommand) -> PluginListCommandResult {
    run(&HostHttp, TRAKT_CLIENT_ID, command).await
}

fn into_result<T>(result: Result<T, PluginError>) -> PluginResult<T> {
    match result {
        Ok(value) => PluginResult::Ok(value),
        Err(error) => PluginResult::Err(error),
    }
}

pub async fn run<H: ListHttp>(
    http: &H,
    client_id: &str,
    command: PluginListCommand,
) -> PluginListCommandResult {
    let client = Client {
        http,
        client_id: client_id.trim(),
    };
    match command {
        PluginListCommand::Fetch(request) => {
            PluginListCommandResult::Fetch(into_result(client.fetch(&request).await))
        }
        PluginListCommand::Account(request) => {
            PluginListCommandResult::Account(into_result(client.account(&request).await))
        }
        PluginListCommand::Health(_) => {
            PluginListCommandResult::Health(PluginResult::Err(plugin_error(
                PluginErrorCode::Unsupported,
                "Trakt has no server key to check",
            )))
        }
    }
}

struct Client<'a, H> {
    http: &'a H,
    client_id: &'a str,
}

/// Where one source's entries live and how to describe it.
struct Target {
    /// The paged items endpoint, without its query.
    items_path: String,
    /// The list summary, read on the first page for the name and address.
    summary_path: Option<String>,
    /// The address to show when the summary gives none.
    site_url: Option<String>,
    /// The member's token, for personal sources only.
    token: Option<String>,
    what: String,
}

fn required(request: &ListPluginFetchRequest, key: &str) -> Result<String, PluginError> {
    request
        .params
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| missing_param(key))
}

fn numeric_list_id(request: &ListPluginFetchRequest) -> Result<String, PluginError> {
    let value = required(request, PARAM_LIST_ID)?;
    // Accept a pasted `1234-some-name`: the id is its leading digits.
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    positive_id(&digits)
        .ok_or_else(|| invalid_config(format!("{PARAM_LIST_ID} must be a Trakt numeric list id")))
}

fn member_token(credential: Option<&ListCredential>) -> Result<String, PluginError> {
    credential
        .map(|credential| credential.access_token.trim())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| auth_failed("connect a Trakt account to follow this list"))
}

/// Lists carry seasons and episodes as well as movies and shows; each maps to
/// its show. The watchlist endpoint for every type at once takes movies and
/// shows only, in the member's own order.
fn target(request: &ListPluginFetchRequest) -> Result<Target, PluginError> {
    const LIST_ITEMS: &str = "items/movie,show,season,episode";
    match request.source_type.as_str() {
        SOURCE_USER_LIST => {
            let user = required(request, PARAM_USER)?;
            let list = required(request, PARAM_LIST)?;
            let path = format!(
                "/users/{}/lists/{}",
                encode_component(&user),
                encode_component(&list)
            );
            Ok(Target {
                items_path: format!("{path}/{LIST_ITEMS}"),
                summary_path: Some(path.clone()),
                site_url: Some(format!("{SITE_BASE}{path}")),
                token: None,
                what: format!("Trakt list {user}/{list}"),
            })
        }
        SOURCE_LIST => {
            let id = numeric_list_id(request)?;
            Ok(Target {
                items_path: format!("/lists/{id}/{LIST_ITEMS}"),
                summary_path: Some(format!("/lists/{id}")),
                site_url: Some(format!("{SITE_BASE}/lists/{id}")),
                token: None,
                what: format!("Trakt list {id}"),
            })
        }
        SOURCE_WATCHLIST => Ok(Target {
            items_path: "/users/me/watchlist/movie,show/rank".to_string(),
            summary_path: None,
            site_url: None,
            token: Some(member_token(request.credential.as_ref())?),
            what: "Trakt watchlist".to_string(),
        }),
        SOURCE_MY_LIST => {
            let list = required(request, PARAM_LIST)?;
            let token = member_token(request.credential.as_ref())?;
            let path = format!("/users/me/lists/{}", encode_component(&list));
            Ok(Target {
                items_path: format!("{path}/{LIST_ITEMS}"),
                summary_path: Some(path),
                site_url: None,
                token: Some(token),
                what: format!("Trakt list {list}"),
            })
        }
        other => Err(unsupported_source(other)),
    }
}

fn header_number(response: &PluginHttpResponse, name: &str) -> Option<u32> {
    header(response, name)?.trim().parse().ok()
}

impl<H: ListHttp> Client<'_, H> {
    fn app(&self) -> Result<(), PluginError> {
        if self.client_id.is_empty() {
            return Err(invalid_config(NO_CLIENT_ID));
        }
        Ok(())
    }

    fn request(&self, path_and_query: &str, token: Option<&str>) -> PluginHttpRequest {
        let mut request = get(format!("{API_BASE}{path_and_query}"), USER_AGENT, JSON);
        let headers = &mut request.headers;
        headers.insert("Content-Type".to_string(), JSON.to_string());
        headers.insert("trakt-api-key".to_string(), self.client_id.to_string());
        headers.insert("trakt-api-version".to_string(), API_VERSION.to_string());
        if let Some(token) = token {
            headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        }
        request
    }

    async fn send(
        &self,
        path_and_query: &str,
        token: Option<&str>,
        what: &str,
    ) -> Result<PluginHttpResponse, PluginError> {
        let response = self.http.send(self.request(path_and_query, token)).await?;
        check_trakt_status(&response, token.is_some(), what)?;
        Ok(response)
    }

    async fn fetch(
        &self,
        request: &ListPluginFetchRequest,
    ) -> Result<ListPluginFetchResponse, PluginError> {
        self.app()?;
        let target = target(request)?;
        let token = target.token.as_deref();
        let page = numeric_cursor(request.page_cursor.as_deref(), 1)?.max(1);

        let mut list_name = None;
        let mut list_url = target.site_url.clone();
        if page == 1
            && let Some(path) = &target.summary_path
        {
            let summary = json_body(&self.send(path, token, &target.what).await?)?;
            list_name = json_text(summary.get("name"));
            list_url = summary_url(&summary).or(list_url);
        }

        let response = self
            .send(
                &format!("{}?page={page}&limit={PAGE_LIMIT}", target.items_path),
                token,
                &target.what,
            )
            .await?;
        let body = json_body(&response)?;
        let entries = body.as_array().ok_or_else(|| {
            permanent(format!(
                "Trakt answered {} with an unexpected document",
                target.what
            ))
        })?;
        let items = merge_entries(entries.iter().filter_map(to_item).collect());

        // A missing page count means Trakt sent everything at once.
        let page_count = header_number(&response, "x-pagination-page-count").unwrap_or(page);
        if page == 1 && page_count <= 1 {
            return Ok(single_page(
                dedupe_and_rank(items, 1),
                list_name,
                list_url,
                request.since_fingerprint.as_deref(),
            ));
        }
        let limit = header_number(&response, "x-pagination-limit")
            .filter(|limit| *limit > 0)
            .unwrap_or(PAGE_LIMIT);
        let last = page_count.clamp(1, MAX_PAGES);
        Ok(ListPluginFetchResponse {
            items: dedupe_and_rank(items, (page - 1).saturating_mul(limit) + 1),
            next_cursor: (page < last).then(|| (page + 1).to_string()),
            list_name,
            list_url,
            total_hint: header_number(&response, "x-pagination-item-count")
                .map(|total| total.min(MAX_PAGES.saturating_mul(limit))),
            // Multi-page sources carry no fingerprint: the host only compares
            // the first page, which cannot vouch for the rest.
            fingerprint: None,
            unchanged: false,
        })
    }

    async fn account(
        &self,
        request: &ListPluginAccountRequest,
    ) -> Result<ListPluginAccountResponse, PluginError> {
        self.app()?;
        let token = member_token(Some(&request.credential))?;
        let settings = json_body(
            &self
                .send("/users/settings", Some(&token), "Trakt account")
                .await?,
        )?;
        let user = settings
            .get("user")
            .ok_or_else(|| permanent("Trakt answered without the account's user"))?;
        let ids = user.get("ids");
        let slug = ids.and_then(|ids| json_text(ids.get("slug")));
        let username = json_text(user.get("username"))
            .or_else(|| slug.clone())
            .ok_or_else(|| permanent("Trakt answered without the account's username"))?;
        // The uuid survives a username change; older payloads carry only the
        // numeric id.
        let external_user_id = ids
            .and_then(|ids| json_text(ids.get("uuid")).or_else(|| json_id(ids.get("trakt"))))
            .or(slug)
            .unwrap_or_else(|| username.clone());
        Ok(ListPluginAccountResponse {
            external_user_id,
            username,
            display_name: json_text(user.get("name")),
            avatar_url: json_text(
                user.get("images")
                    .and_then(|images| images.get("avatar"))
                    .and_then(|avatar| avatar.get("full")),
            ),
            owned_lists: self.owned_lists(&token).await?,
            statuses: Vec::new(),
        })
    }

    async fn owned_lists(&self, token: &str) -> Result<Vec<ListAccountList>, PluginError> {
        let mut lists = Vec::new();
        let mut page = 1;
        loop {
            let response = self
                .send(
                    &format!("/users/me/lists?page={page}&limit={ACCOUNT_LIST_LIMIT}"),
                    Some(token),
                    "Trakt lists",
                )
                .await?;
            for entry in json_body(&response)?.as_array().into_iter().flatten() {
                let ids = entry.get("ids");
                let Some(id) = ids.and_then(|ids| {
                    json_id(ids.get("trakt")).or_else(|| json_text(ids.get("slug")))
                }) else {
                    continue;
                };
                lists.push(ListAccountList {
                    name: json_text(entry.get("name")).unwrap_or_else(|| id.clone()),
                    id,
                    kinds: vec![ListMediaKind::Movie, ListMediaKind::Series],
                });
            }
            let page_count = header_number(&response, "x-pagination-page-count").unwrap_or(page);
            if page >= page_count.min(MAX_ACCOUNT_LIST_PAGES) {
                return Ok(lists);
            }
            page += 1;
        }
    }
}

/// Trakt's documented status codes onto the host's failure classes. A 401
/// is a lapsed member token when one was sent, and a private list when the
/// request was anonymous; a 403 is the app's client id itself.
fn check_trakt_status(
    response: &PluginHttpResponse,
    authorized: bool,
    what: &str,
) -> Result<(), PluginError> {
    match response.status {
        401 if authorized => Err(auth_failed(
            "Trakt rejected the account token; reconnect the Trakt account",
        )),
        401 => Err(not_found(format!("{what} (private, HTTP 401)"))),
        403 => Err(invalid_config(
            "Trakt rejected the plugin's app client id (HTTP 403)",
        )),
        420 => Err(permanent(format!(
            "{what} is over a Trakt account limit (HTTP 420)"
        ))),
        423 => Err(permanent(
            "the Trakt account is locked; its owner must contact Trakt support (HTTP 423)",
        )),
        426 => Err(permanent(format!(
            "{what} needs a Trakt VIP account (HTTP 426)"
        ))),
        _ => check_status(response, Access::Public, what),
    }
}

/// The list's own address: the owner's slug and list slug when Trakt gives
/// both, otherwise the numeric list address.
fn summary_url(summary: &Value) -> Option<String> {
    let user = summary
        .get("user")
        .and_then(|user| user.get("ids"))
        .and_then(|ids| json_text(ids.get("slug")));
    let ids = summary.get("ids");
    match (user, ids.and_then(|ids| json_text(ids.get("slug")))) {
        (Some(user), Some(list)) => Some(format!(
            "{SITE_BASE}/users/{}/lists/{}",
            encode_component(&user),
            encode_component(&list)
        )),
        _ => ids
            .and_then(|ids| json_id(ids.get("trakt")))
            .map(|id| format!("{SITE_BASE}/lists/{id}")),
    }
}

fn season_number(value: Option<&Value>, key: &str) -> Option<i32> {
    value?
        .get(key)?
        .as_i64()
        .filter(|season| *season >= 0)
        .and_then(|season| i32::try_from(season).ok())
}

/// Map one listed entry. Movies and shows map directly; a season or an
/// episode maps to its show and notes the season. People, and any type Trakt
/// adds later, are skipped.
fn to_item(entry: &Value) -> Option<ListPluginItem> {
    let (kind, media, season) = match entry.get("type").and_then(Value::as_str)? {
        "movie" => (ListMediaKind::Movie, entry.get("movie")?, None),
        "show" => (ListMediaKind::Series, entry.get("show")?, None),
        "season" => (
            ListMediaKind::Series,
            entry.get("show")?,
            season_number(entry.get("season"), "number"),
        ),
        "episode" => (
            ListMediaKind::Series,
            entry.get("show")?,
            season_number(entry.get("episode"), "season"),
        ),
        _ => return None,
    };
    let kind_name = kind_str(kind)?;
    let media_ids = media.get("ids");
    let id = |key: &str| media_ids.and_then(|ids| ids.get(key));
    let ids = Ids::default()
        .with_tmdb(json_id(id("tmdb")))
        .with_imdb(json_text(id("imdb")))
        .with_tvdb(json_id(id("tvdb")));
    let trakt = json_id(id("trakt"));
    let title = json_text(media.get("title"));
    let year = json_year(media.get("year"));
    let mut item = match &trakt {
        // With no shared id, the Trakt id is still steadier than the title.
        Some(trakt) if ids.is_empty() => ListPluginItem {
            item_key: format!("{SOURCE_TRAKT}:{kind_name}:{trakt}"),
            kind_hint: Some(kind),
            title,
            year,
            ..ListPluginItem::default()
        },
        _ => build_item(&ids, Some(kind), title, year)?,
    };
    if let Some(trakt) = trakt {
        item.external_ids.push(ListExternalId {
            source: SOURCE_TRAKT.to_string(),
            kind: Some(kind_name.to_string()),
            id: trakt,
        });
    }
    item.season = season;
    Some(item)
}

/// Fold entries that name the same title into the first, in list order. The
/// season survives only when every entry for a show names the same one; a
/// show that is also listed whole, or under several seasons, is followed
/// whole.
fn merge_entries(items: Vec<ListPluginItem>) -> Vec<ListPluginItem> {
    let mut out: Vec<ListPluginItem> = Vec::with_capacity(items.len());
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for item in items {
        match seen.get(&item.item_key) {
            Some(&index) => {
                if out[index].season != item.season {
                    out[index].season = None;
                }
            }
            None => {
                seen.insert(item.item_key.clone(), out.len());
                out.push(item);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
