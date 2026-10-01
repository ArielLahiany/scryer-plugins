use std::collections::BTreeMap;

use list_provider_common::testing::{RecordedHttp, block_on};
use scryer_plugin_sdk::command::{PluginListCommand, PluginListCommandResult};
use scryer_plugin_sdk::host::PluginHttpRequest;
use scryer_plugin_sdk::{
    ListCredential, ListMediaKind, ListPluginAccountRequest, ListPluginFetchRequest,
    ListPluginHealthRequest, PluginDescriptor, PluginErrorCode, PluginResult,
};

use super::*;

const CLIENT: &str = "fixture-client-id-0001";
const TOKEN: &str = "fixture-access-token-0001";

fn api(path_and_query: &str) -> String {
    format!("{API_BASE}{path_and_query}")
}

fn items_url(list_path: &str, page: u32) -> String {
    api(&format!(
        "{list_path}/items/movie,show,season,episode?page={page}&limit=250"
    ))
}

fn credential() -> ListCredential {
    ListCredential {
        access_token: TOKEN.to_string(),
        token_type: Some("bearer".to_string()),
        external_user_id: None,
        username: Some("fixture-member".to_string()),
    }
}

fn request(
    source_type: &str,
    params: &[(&str, &str)],
    cursor: Option<&str>,
) -> ListPluginFetchRequest {
    ListPluginFetchRequest {
        source_type: source_type.to_string(),
        params: params
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>(),
        credential: None,
        page_cursor: cursor.map(str::to_string),
        since_fingerprint: None,
    }
}

fn member_request(
    source_type: &str,
    params: &[(&str, &str)],
    cursor: Option<&str>,
) -> ListPluginFetchRequest {
    ListPluginFetchRequest {
        credential: Some(credential()),
        ..request(source_type, params, cursor)
    }
}

fn fetch(
    http: &RecordedHttp,
    client_id: &str,
    request: ListPluginFetchRequest,
) -> PluginResult<scryer_plugin_sdk::ListPluginFetchResponse> {
    match block_on(run(http, client_id, PluginListCommand::Fetch(request))) {
        PluginListCommandResult::Fetch(result) => result,
        other => panic!("unexpected {other:?}"),
    }
}

fn account(
    http: &RecordedHttp,
    client_id: &str,
) -> PluginResult<scryer_plugin_sdk::ListPluginAccountResponse> {
    match block_on(run(
        http,
        client_id,
        PluginListCommand::Account(ListPluginAccountRequest {
            credential: credential(),
        }),
    )) {
        PluginListCommandResult::Account(result) => result,
        other => panic!("unexpected {other:?}"),
    }
}

fn ok<T: std::fmt::Debug>(result: PluginResult<T>) -> T {
    match result {
        PluginResult::Ok(value) => value,
        PluginResult::Err(error) => panic!("unexpected error {error:?}"),
    }
}

fn err<T: std::fmt::Debug>(result: PluginResult<T>) -> scryer_plugin_sdk::PluginError {
    match result {
        PluginResult::Err(error) => error,
        PluginResult::Ok(value) => panic!("unexpected success {value:?}"),
    }
}

fn sent_header<'a>(request: &'a PluginHttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn keys(response: &scryer_plugin_sdk::ListPluginFetchResponse) -> Vec<&str> {
    response
        .items
        .iter()
        .map(|item| item.item_key.as_str())
        .collect()
}

const USER_LIST_SUMMARY: &str = r#"{
  "name": "Fixture Picks", "description": "Synthetic fixture list", "privacy": "public",
  "share_link": "https://trakt.tv/lists/7700001", "type": "personal", "display_numbers": true,
  "allow_comments": true, "sort_by": "rank", "sort_how": "asc",
  "created_at": "2030-01-01T00:00:00.000Z", "updated_at": "2031-01-01T00:00:00.000Z",
  "item_count": 260, "comment_count": 0, "likes": 0,
  "ids": {"trakt": 7700001, "slug": "fixture-picks"},
  "user": {"username": "Fixture User", "private": false, "deleted": false, "name": "Fixture User",
           "vip": false, "vip_ep": false, "ids": {"slug": "fixture-user", "trakt": 5500001}}
}"#;

const USER_LIST_PAGE_1: &str = r#"[
  {"rank": 1, "id": 101, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "movie",
   "movie": {"title": "Fixture Feature Alpha", "year": 2031,
             "ids": {"trakt": 900001, "slug": "fixture-feature-alpha-2031", "imdb": "tt0000001", "tmdb": 990001}}},
  {"rank": 2, "id": 102, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "show",
   "show": {"title": "Fixture Serial Beta", "year": 2029,
            "ids": {"trakt": 900002, "slug": "fixture-serial-beta", "tvdb": 880002, "imdb": null, "tmdb": null}}},
  {"rank": 3, "id": 103, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "season",
   "season": {"number": 2, "ids": {"trakt": 910003, "tvdb": 870003, "tmdb": 860003}},
   "show": {"title": "Fixture Serial Gamma", "year": 2027,
            "ids": {"trakt": 900003, "slug": "fixture-serial-gamma", "tvdb": 880003, "imdb": "tt0000003", "tmdb": 990003}}},
  {"rank": 4, "id": 104, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "episode",
   "episode": {"season": 1, "number": 4, "title": "Fixture Episode",
               "ids": {"trakt": 920004, "tvdb": 850004, "imdb": null, "tmdb": 840004}},
   "show": {"title": "Fixture Serial Delta", "year": 2030,
            "ids": {"trakt": 900004, "slug": "fixture-serial-delta", "tvdb": 880004, "imdb": null, "tmdb": 990004}}},
  {"rank": 5, "id": 105, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "person",
   "person": {"name": "Fixture Person", "ids": {"trakt": 930005, "slug": "fixture-person"}}},
  {"rank": 6, "id": 106, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "movie",
   "movie": {"title": "Fixture Feature Epsilon", "year": 2032,
             "ids": {"trakt": 900006, "slug": "fixture-feature-epsilon-2032", "imdb": null, "tmdb": null}}},
  {"rank": 7, "id": 107, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "show",
   "show": {"title": "Fixture Serial Gamma", "year": 2027,
            "ids": {"trakt": 900003, "slug": "fixture-serial-gamma", "tvdb": 880003, "imdb": "tt0000003", "tmdb": 990003}}}
]"#;

const USER_LIST_PAGE_2: &str = r#"[
  {"rank": 251, "id": 351, "listed_at": "2031-01-01T00:00:00.000Z", "notes": null, "type": "movie",
   "movie": {"title": "Fixture Feature Zeta", "year": 2033,
             "ids": {"trakt": 900251, "slug": "fixture-feature-zeta-2033", "imdb": "tt0000251", "tmdb": 990251}}}
]"#;

fn paged_headers(page: &'static str) -> [(&'static str, &'static str); 4] {
    [
        ("X-Pagination-Page", page),
        ("X-Pagination-Limit", "250"),
        ("X-Pagination-Page-Count", "2"),
        ("X-Pagination-Item-Count", "260"),
    ]
}

#[test]
fn descriptor_round_trips_and_passes_host_checks() {
    let original = descriptor();
    let decoded: PluginDescriptor =
        serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    scryer_plugin_sdk::validate_plugin_descriptor_sdk_contract(
        &decoded,
        scryer_plugin_sdk::SDK_VERSION,
    )
    .unwrap();
    scryer_plugin_sdk::validate_plugin_descriptor_host_permissions(&decoded).unwrap();

    let list = decoded.list_provider().unwrap();
    assert_eq!(decoded.id, "trakt-list");
    assert_eq!(list.provider_type, "trakt");
    assert_eq!(
        list.auth,
        ListProviderAuth::MemberAccount {
            flow: ListAccountFlow::AuthorizationCode { pkce: false },
            exchange: ListAccountExchange::SmgRelay,
            byo_app: true,
            scopes: Vec::new(),
        }
    );
    assert!(list.capabilities.account);
    assert!(!list.capabilities.health);
    assert!(!list.capabilities.requires_member_credential);
    assert!(list.config_fields.is_empty());
    assert_eq!(list.allowed_hosts, vec!["api.trakt.tv".to_string()]);
    assert_eq!(list.rate_limit_seconds, Some(2));

    let sources: Vec<_> = list
        .groups
        .iter()
        .flat_map(|group| &group.items)
        .map(|item| (item.source_type.as_str(), item.personal))
        .collect();
    assert_eq!(
        sources,
        vec![
            ("user_list", false),
            ("list", false),
            ("watchlist", true),
            ("my_list", true),
        ]
    );
    assert_eq!(
        list.groups[0].auth_badge,
        ListAuthBadge::NoAccountNeedsValue
    );
    assert_eq!(list.groups[1].auth_badge, ListAuthBadge::MemberAccount);
    assert!(
        list.groups
            .iter()
            .flat_map(|group| &group.items)
            .all(|item| item.default_interval_seconds == 12 * 60 * 60)
    );
    // Every pattern lands on a public source, and every capture targets one
    // of its parameters.
    for pattern in &list.url_patterns {
        let item = list.groups[0]
            .items
            .iter()
            .find(|item| item.source_type == pattern.source_type)
            .unwrap();
        for capture in &pattern.captures {
            assert!(item.params.iter().any(|param| param.key == capture.param));
        }
    }
}

#[test]
fn url_patterns_extract_users_lists_and_ids_from_site_addresses() {
    let list = descriptor().list_provider().cloned().unwrap();
    let recognise = |address: &str| {
        list.url_patterns.iter().find_map(|pattern| {
            let regex = regex::Regex::new(&pattern.pattern).unwrap();
            regex.captures(address).map(|captures| {
                let params: Vec<(String, String)> = pattern
                    .captures
                    .iter()
                    .map(|capture| {
                        (
                            capture.param.clone(),
                            captures[capture.group.as_str()].to_string(),
                        )
                    })
                    .collect();
                (pattern.source_type.clone(), params)
            })
        })
    };
    let pairs = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        recognise("https://trakt.tv/users/fixture-user/lists/fixture-picks"),
        Some((
            "user_list".to_string(),
            pairs(&[("user", "fixture-user"), ("list", "fixture-picks")])
        ))
    );
    assert_eq!(
        recognise("https://app.trakt.tv/users/fixture-user/lists/7700001/?sort=rank,asc"),
        Some((
            "user_list".to_string(),
            pairs(&[("user", "fixture-user"), ("list", "7700001")])
        ))
    );
    assert_eq!(
        recognise("https://www.trakt.tv/lists/7700002"),
        Some(("list".to_string(), pairs(&[("list_id", "7700002")])))
    );
    assert_eq!(
        recognise("http://trakt.tv/lists/7700002-fixture-collection#top"),
        Some(("list".to_string(), pairs(&[("list_id", "7700002")])))
    );
    for address in [
        "https://trakt.tv/users/fixture-user/watchlist",
        "https://trakt.tv/users/fixture-user/lists",
        "https://trakt.tv/users/fixture-user/lists/fixture-picks/comments",
        "https://trakt.tv/movies/fixture-feature-alpha-2031",
        "https://trakt.tv/lists/trending",
        "https://trakt.tv.example.test/lists/7700002",
    ] {
        assert_eq!(recognise(address), None, "{address}");
    }
}

#[test]
fn public_user_list_pages_through_trakt_headers_anonymously() {
    let list_path = "/users/fixture-user/lists/fixture-picks";
    let http = RecordedHttp::new()
        .with(&api(list_path), 200, USER_LIST_SUMMARY)
        .with_headers(
            &items_url(list_path, 1),
            200,
            &paged_headers("1"),
            USER_LIST_PAGE_1,
        )
        .with_headers(
            &items_url(list_path, 2),
            200,
            &paged_headers("2"),
            USER_LIST_PAGE_2,
        );
    // A public source never forwards a member token, even when one is at
    // hand, so a lapsed link cannot break it.
    let first = ok(fetch(
        &http,
        CLIENT,
        member_request(
            "user_list",
            &[("user", "fixture-user"), ("list", "fixture-picks")],
            None,
        ),
    ));
    for sent in http.requests() {
        assert_eq!(sent_header(&sent, "trakt-api-key"), Some(CLIENT));
        assert_eq!(sent_header(&sent, "trakt-api-version"), Some("2"));
        assert_eq!(sent_header(&sent, "Content-Type"), Some("application/json"));
        assert_eq!(sent_header(&sent, "Authorization"), None);
        assert!(!sent.url.contains(CLIENT));
    }
    assert_eq!(first.list_name.as_deref(), Some("Fixture Picks"));
    assert_eq!(
        first.list_url.as_deref(),
        Some("https://trakt.tv/users/fixture-user/lists/fixture-picks")
    );
    assert_eq!(first.next_cursor.as_deref(), Some("2"));
    assert_eq!(first.total_hint, Some(260));
    assert!(first.fingerprint.is_none());
    assert_eq!(
        keys(&first),
        vec![
            "tmdb:movie:990001",
            "tvdb:series:880002",
            "tmdb:series:990003",
            "tmdb:series:990004",
            "trakt:movie:900006",
        ],
        "the person is skipped and the show listed twice is folded"
    );
    let ranks: Vec<_> = first.items.iter().map(|item| item.rank).collect();
    assert_eq!(ranks, vec![Some(1), Some(2), Some(3), Some(4), Some(5)]);

    let movie = &first.items[0];
    assert_eq!(movie.kind_hint, Some(ListMediaKind::Movie));
    assert_eq!(movie.title.as_deref(), Some("Fixture Feature Alpha"));
    assert_eq!(movie.year, Some(2031));
    let ids: Vec<_> = movie
        .external_ids
        .iter()
        .map(|id| (id.source.as_str(), id.kind.as_deref(), id.id.as_str()))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("tmdb", Some("movie"), "990001"),
            ("imdb", Some("movie"), "tt0000001"),
            ("trakt", Some("movie"), "900001"),
        ]
    );

    let show = &first.items[1];
    assert_eq!(show.kind_hint, Some(ListMediaKind::Series));
    assert_eq!(show.season, None);
    assert!(
        show.external_ids
            .iter()
            .any(|id| id.source == "trakt" && id.kind.as_deref() == Some("series"))
    );
    assert_eq!(
        first.items[2].season, None,
        "a show listed whole and by season is followed whole"
    );
    assert_eq!(
        first.items[3].season,
        Some(1),
        "an episode maps to its show and season"
    );
    let trakt_only = &first.items[4];
    assert_eq!(trakt_only.title.as_deref(), Some("Fixture Feature Epsilon"));
    assert_eq!(trakt_only.external_ids.len(), 1);

    let second = ok(fetch(
        &http,
        CLIENT,
        request(
            "user_list",
            &[("user", "fixture-user"), ("list", "fixture-picks")],
            first.next_cursor.as_deref(),
        ),
    ));
    assert!(second.next_cursor.is_none());
    assert_eq!(second.items[0].item_key, "tmdb:movie:990251");
    assert_eq!(second.items[0].rank, Some(251));
    assert_eq!(
        second.list_url.as_deref(),
        Some("https://trakt.tv/users/fixture-user/lists/fixture-picks")
    );
    assert_eq!(
        http.urls()
            .iter()
            .filter(|url| *url == &api(list_path))
            .count(),
        1,
        "the summary is read on the first page only"
    );
}

#[test]
fn a_season_listed_alone_keeps_its_season() {
    let body = r#"[
      {"rank": 1, "id": 201, "listed_at": "2031-01-01T00:00:00.000Z", "type": "season",
       "season": {"number": 3, "ids": {"trakt": 910201}},
       "show": {"title": "Fixture Serial Eta", "year": 2026, "ids": {"trakt": 900201, "slug": "fixture-serial-eta", "tmdb": 990201}}},
      {"rank": 2, "id": 202, "listed_at": "2031-01-01T00:00:00.000Z", "type": "season",
       "season": {"number": 1, "ids": {"trakt": 910202}},
       "show": {"title": "Fixture Serial Theta", "year": 2025, "ids": {"trakt": 900202, "slug": "fixture-serial-theta", "tmdb": 990202}}},
      {"rank": 3, "id": 203, "listed_at": "2031-01-01T00:00:00.000Z", "type": "season",
       "season": {"number": 2, "ids": {"trakt": 910203}},
       "show": {"title": "Fixture Serial Theta", "year": 2025, "ids": {"trakt": 900202, "slug": "fixture-serial-theta", "tmdb": 990202}}}
    ]"#;
    let http = RecordedHttp::new().with(&items_url("/lists/7700009", 2), 200, body);
    let page = ok(fetch(
        &http,
        CLIENT,
        request("list", &[("list_id", "7700009")], Some("2")),
    ));
    let seasons: Vec<_> = page
        .items
        .iter()
        .map(|item| (item.item_key.as_str(), item.season))
        .collect();
    assert_eq!(
        seasons,
        vec![
            ("tmdb:series:990201", Some(3)),
            ("tmdb:series:990202", None)
        ],
        "two seasons of one show widen to the whole show"
    );
}

#[test]
fn single_page_list_by_id_is_fingerprinted() {
    let summary = r#"{
      "name": "Fixture Collection", "privacy": "public", "share_link": "https://trakt.tv/lists/7700002",
      "type": "official", "item_count": 2, "ids": {"trakt": 7700002, "slug": "fixture-collection"},
      "user": {"username": "Fixture Official", "private": false, "deleted": false, "ids": {"slug": null, "trakt": 5500002}}
    }"#;
    let body = r#"[
      {"rank": 1, "id": 301, "listed_at": "2031-01-01T00:00:00.000Z", "type": "movie",
       "movie": {"title": "Fixture Collection One", "year": 2020, "ids": {"trakt": 900301, "slug": "fixture-collection-one-2020", "imdb": "tt0000301", "tmdb": 990301}}},
      {"rank": 2, "id": 302, "listed_at": "2031-01-01T00:00:00.000Z", "type": "movie",
       "movie": {"title": "Fixture Collection Two", "year": 2022, "ids": {"trakt": 900302, "slug": "fixture-collection-two-2022", "imdb": "tt0000302", "tmdb": 990302}}}
    ]"#;
    let headers = [
        ("X-Pagination-Page", "1"),
        ("X-Pagination-Limit", "250"),
        ("X-Pagination-Page-Count", "1"),
        ("X-Pagination-Item-Count", "2"),
    ];
    let http = RecordedHttp::new()
        .with(&api("/lists/7700002"), 200, summary)
        .with_headers(&items_url("/lists/7700002", 1), 200, &headers, body);

    let first = ok(fetch(
        &http,
        CLIENT,
        request("list", &[("list_id", "7700002-fixture-collection")], None),
    ));
    assert_eq!(keys(&first), vec!["tmdb:movie:990301", "tmdb:movie:990302"]);
    assert!(first.next_cursor.is_none());
    assert_eq!(first.list_name.as_deref(), Some("Fixture Collection"));
    assert_eq!(
        first.list_url.as_deref(),
        Some("https://trakt.tv/lists/7700002"),
        "without the owner's slug the numeric address is used"
    );
    assert!(first.fingerprint.is_some());

    let mut again = request("list", &[("list_id", "7700002")], None);
    again.since_fingerprint = first.fingerprint.clone();
    let unchanged = ok(fetch(&http, CLIENT, again));
    assert!(unchanged.unchanged);
    assert!(unchanged.items.is_empty());
}

#[test]
fn watchlist_reads_the_members_watchlist_with_the_bearer_token() {
    let body = r#"[
      {"rank": 1, "id": 401, "listed_at": "2031-02-01T00:00:00.000Z", "notes": null, "type": "show",
       "show": {"title": "Fixture Serial Iota", "year": 2031, "ids": {"trakt": 900401, "slug": "fixture-serial-iota", "tvdb": 880401, "imdb": "tt0000401", "tmdb": 990401}}},
      {"rank": 2, "id": 402, "listed_at": "2031-02-02T00:00:00.000Z", "notes": null, "type": "movie",
       "movie": {"title": "Fixture Feature Kappa", "year": 2032, "ids": {"trakt": 900402, "slug": "fixture-feature-kappa-2032", "imdb": "tt0000402", "tmdb": 990402}}}
    ]"#;
    let url = api("/users/me/watchlist/movie,show/rank?page=1&limit=250");
    let http = RecordedHttp::new().with(&url, 200, body);
    let watchlist = ok(fetch(&http, CLIENT, member_request("watchlist", &[], None)));
    assert_eq!(
        keys(&watchlist),
        vec!["tmdb:series:990401", "tmdb:movie:990402"]
    );
    assert!(
        watchlist.fingerprint.is_some(),
        "no page count means one page"
    );
    assert!(watchlist.list_name.is_none());

    let sent = http.requests();
    assert_eq!(sent.len(), 1, "the watchlist has no summary to read");
    assert_eq!(
        sent_header(&sent[0], "Authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(sent_header(&sent[0], "trakt-api-key"), Some(CLIENT));
    assert!(!sent[0].url.contains(TOKEN));

    let anonymous = RecordedHttp::new();
    let error = err(fetch(&anonymous, CLIENT, request("watchlist", &[], None)));
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
    assert!(anonymous.urls().is_empty());
}

#[test]
fn own_list_reads_the_members_list_with_the_bearer_token() {
    let summary = r#"{
      "name": "Fixture Private Shelf", "privacy": "private", "share_link": "https://trakt.tv/lists/7700003",
      "type": "personal", "item_count": 1, "ids": {"trakt": 7700003, "slug": "fixture-private-shelf"},
      "user": {"username": "fixture-member", "private": true, "deleted": false, "ids": {"slug": "fixture-member", "trakt": 5500003}}
    }"#;
    let body = r#"[
      {"rank": 1, "id": 501, "listed_at": "2031-03-01T00:00:00.000Z", "type": "movie",
       "movie": {"title": "Fixture Feature Lambda", "year": 2030, "ids": {"trakt": 900501, "slug": "fixture-feature-lambda-2030", "imdb": "tt0000501", "tmdb": 990501}}}
    ]"#;
    let http = RecordedHttp::new()
        .with(&api("/users/me/lists/7700003"), 200, summary)
        .with_headers(
            &items_url("/users/me/lists/7700003", 1),
            200,
            &[("X-Pagination-Page-Count", "1")],
            body,
        );
    let own = ok(fetch(
        &http,
        CLIENT,
        member_request("my_list", &[("list", "7700003")], None),
    ));
    assert_eq!(keys(&own), vec!["tmdb:movie:990501"]);
    assert_eq!(own.list_name.as_deref(), Some("Fixture Private Shelf"));
    assert_eq!(
        own.list_url.as_deref(),
        Some("https://trakt.tv/users/fixture-member/lists/fixture-private-shelf")
    );
    for sent in http.requests() {
        assert_eq!(
            sent_header(&sent, "Authorization"),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }

    let empty = RecordedHttp::new();
    assert_eq!(
        err(fetch(&empty, CLIENT, member_request("my_list", &[], None))).code,
        PluginErrorCode::InvalidConfig
    );
    assert_eq!(
        err(fetch(
            &empty,
            CLIENT,
            request("my_list", &[("list", "7700003")], None)
        ))
        .code,
        PluginErrorCode::AuthFailed
    );
    assert!(empty.urls().is_empty());
}

#[test]
fn account_reports_the_member_and_their_own_lists() {
    let settings = r#"{
      "user": {"email": "fixture-member@example.test", "username": "fixture-member", "private": false,
               "name": "Fixture Member", "vip": false, "vip_ep": false, "director": false,
               "ids": {"slug": "fixture-member", "trakt": null, "uuid": "00000000-0000-4000-8000-000000000001"},
               "joined_at": "2030-01-01T00:00:00.000Z",
               "images": {"avatar": {"full": "https://media.example.test/avatars/fixture-member.jpg"}},
               "vip_og": false, "vip_years": 0},
      "permissions": {"commenting": true, "liking": true, "following": true},
      "account": {"timezone": "UTC", "date_format": "dd/mm/yyyy", "time_24hr": true}
    }"#;
    let lists_page_1 = r#"[
      {"name": "Fixture Shelf One", "privacy": "private", "type": "personal", "item_count": 3,
       "ids": {"trakt": 7700011, "slug": "fixture-shelf-one"}},
      {"name": "Fixture Shelf Two", "privacy": "public", "type": "personal", "item_count": 9,
       "ids": {"trakt": 7700012, "slug": "fixture-shelf-two"}}
    ]"#;
    let lists_page_2 = r#"[
      {"name": "Fixture Shelf Three", "privacy": "friends", "type": "personal", "item_count": 0,
       "ids": {"trakt": 7700013, "slug": "fixture-shelf-three"}}
    ]"#;
    let http = RecordedHttp::new()
        .with(&api("/users/settings"), 200, settings)
        .with_headers(
            &api("/users/me/lists?page=1&limit=100"),
            200,
            &[("X-Pagination-Page-Count", "2")],
            lists_page_1,
        )
        .with_headers(
            &api("/users/me/lists?page=2&limit=100"),
            200,
            &[("X-Pagination-Page-Count", "2")],
            lists_page_2,
        );
    let member = ok(account(&http, CLIENT));
    assert_eq!(
        member.external_user_id,
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(member.username, "fixture-member");
    assert_eq!(member.display_name.as_deref(), Some("Fixture Member"));
    assert_eq!(
        member.avatar_url.as_deref(),
        Some("https://media.example.test/avatars/fixture-member.jpg")
    );
    let lists: Vec<_> = member
        .owned_lists
        .iter()
        .map(|list| (list.id.as_str(), list.name.as_str()))
        .collect();
    assert_eq!(
        lists,
        vec![
            ("7700011", "Fixture Shelf One"),
            ("7700012", "Fixture Shelf Two"),
            ("7700013", "Fixture Shelf Three"),
        ]
    );
    assert!(member.statuses.is_empty());
    let sent = http.requests();
    assert_eq!(sent.len(), 3);
    for request in &sent {
        assert_eq!(
            sent_header(request, "Authorization"),
            Some(format!("Bearer {TOKEN}").as_str())
        );
        assert!(!request.url.contains(TOKEN));
    }

    let lapsed = RecordedHttp::new().with(&api("/users/settings"), 401, "");
    let error = err(account(&lapsed, CLIENT));
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
    assert!(!error.public_message.contains(TOKEN));
}

#[test]
fn errors_map_to_host_failure_classes() {
    let public_page = items_url("/lists/7700002", 2);
    let public = |status: u16, headers: &[(&str, &str)], body: &str| {
        let http = RecordedHttp::new().with_headers(&public_page, status, headers, body);
        err(fetch(
            &http,
            CLIENT,
            request("list", &[("list_id", "7700002")], Some("2")),
        ))
    };
    let watchlist_page = api("/users/me/watchlist/movie,show/rank?page=1&limit=250");
    let member = |status: u16| {
        let http = RecordedHttp::new().with(&watchlist_page, status, "");
        err(fetch(&http, CLIENT, member_request("watchlist", &[], None)))
    };

    let private = public(401, &[], "");
    assert_eq!(private.code, PluginErrorCode::Permanent);
    assert!(private.public_message.contains("not found"));
    let lapsed = member(401);
    assert_eq!(lapsed.code, PluginErrorCode::AuthFailed);
    assert!(!lapsed.public_message.contains(TOKEN));
    for app_rejected in [public(403, &[], ""), member(403)] {
        assert_eq!(app_rejected.code, PluginErrorCode::InvalidConfig);
    }
    let missing = public(404, &[], "");
    assert_eq!(missing.code, PluginErrorCode::Permanent);
    assert!(missing.public_message.contains("not found"));
    let limited = public(429, &[("Retry-After", "30")], "");
    assert_eq!(
        (limited.code, limited.retry_after_seconds),
        (PluginErrorCode::RateLimited, Some(30))
    );
    let locked = member(423);
    assert_eq!(locked.code, PluginErrorCode::Permanent);
    assert!(!locked.public_message.contains("not found"));
    assert_eq!(member(426).code, PluginErrorCode::Permanent);
    for status in [500, 503, 520] {
        assert_eq!(
            public(status, &[], "").code,
            PluginErrorCode::UpstreamUnavailable
        );
    }
    assert_eq!(
        public(200, &[], "not json").code,
        PluginErrorCode::Permanent
    );
    assert_eq!(
        public(200, &[], r#"{"error": "unexpected"}"#).code,
        PluginErrorCode::Permanent
    );

    let http = RecordedHttp::new();
    let code = |request: ListPluginFetchRequest| err(fetch(&http, CLIENT, request)).code;
    assert_eq!(
        code(request("user_list", &[("user", "fixture-user")], None)),
        PluginErrorCode::InvalidConfig
    );
    assert_eq!(
        code(request("list", &[("list_id", "fixture")], None)),
        PluginErrorCode::InvalidConfig
    );
    assert_eq!(
        code(request("list", &[("list_id", "0")], None)),
        PluginErrorCode::InvalidConfig
    );
    assert_eq!(
        code(request("list", &[("list_id", "1")], Some("next"))),
        PluginErrorCode::Permanent
    );
    assert_eq!(
        code(request("trending", &[], None)),
        PluginErrorCode::Unsupported
    );
    assert!(http.urls().is_empty());

    let health = match block_on(run(
        &http,
        CLIENT,
        PluginListCommand::Health(ListPluginHealthRequest {}),
    )) {
        PluginListCommandResult::Health(result) => err(result),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(health.code, PluginErrorCode::Unsupported);
}

#[test]
fn a_missing_client_id_fails_cleanly_without_calling_trakt() {
    let http = RecordedHttp::new();
    for client_id in ["", "   "] {
        let fetched = err(fetch(
            &http,
            client_id,
            member_request("watchlist", &[], None),
        ));
        assert_eq!(fetched.code, PluginErrorCode::InvalidConfig);
        assert!(fetched.public_message.contains("client id"));
        assert_eq!(
            err(fetch(
                &http,
                client_id,
                request("list", &[("list_id", "7700002")], None)
            ))
            .code,
            PluginErrorCode::InvalidConfig
        );
        assert_eq!(
            err(account(&http, client_id)).code,
            PluginErrorCode::InvalidConfig
        );
    }
    assert!(http.urls().is_empty());
}
