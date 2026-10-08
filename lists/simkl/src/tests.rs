use std::collections::BTreeMap;

use list_provider_common::testing::{RecordedHttp, block_on};
use scryer_plugin_sdk::command::{PluginListCommand, PluginListCommandResult};
use scryer_plugin_sdk::{
    ListPluginAccountRequest, ListPluginHealthRequest, PluginDescriptor, PluginErrorCode,
    PluginResult,
};

use super::*;

const CLIENT_ID: &str = "fixture-client-id";
const TOKEN: &str = "simkl_at_fixture0token0value";

const ACTIVITIES: &str = r#"{
  "all": "2035-03-01T10:00:00Z",
  "settings": { "all": "2035-01-01T10:00:00Z" },
  "tv_shows": {
    "all": "2035-02-01T10:00:00Z",
    "rated_at": null,
    "playback": null,
    "plantowatch": "2035-02-01T10:00:00Z",
    "watching": "2035-01-20T10:00:00Z",
    "completed": null,
    "hold": null,
    "dropped": null,
    "removed_from_list": null
  },
  "anime": {
    "all": "2035-02-02T10:00:00Z",
    "rated_at": null,
    "playback": null,
    "plantowatch": null,
    "watching": "2035-02-02T10:00:00Z",
    "completed": null,
    "hold": null,
    "dropped": null,
    "removed_from_list": null
  },
  "movies": {
    "all": "2035-02-03T10:00:00Z",
    "rated_at": null,
    "playback": null,
    "plantowatch": "2035-02-03T10:00:00Z",
    "completed": null,
    "dropped": null,
    "removed_from_list": null
  },
  "custom_lists": { "lists": { "all": null } }
}"#;

const SHOWS: &str = r#"{
  "shows": [
    {
      "added_to_watchlist_at": "2035-01-02T03:04:05Z",
      "last_watched_at": null,
      "user_rated_at": null,
      "user_rating": null,
      "status": "watching",
      "last_watched": null,
      "next_to_watch": "S01E01",
      "watched_episodes_count": 0,
      "total_episodes_count": 10,
      "not_aired_episodes_count": 0,
      "show": {
        "title": "Fixture Serial One",
        "poster": "00/0000fixture1",
        "year": 2031,
        "ids": {
          "simkl": 9900101,
          "slug": "fixture-serial-one",
          "imdb": "tt9900101",
          "tvdb": "880101",
          "tmdb": "770101"
        }
      }
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Serial Without Simkl Id",
        "year": 2032,
        "ids": { "tvdb": 880102 }
      }
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Serial One",
        "year": 2031,
        "ids": { "simkl": 9900101, "slug": "fixture-serial-one" }
      }
    }
  ]
}"#;

const ANIME: &str = r#"{
  "anime": [
    {
      "added_to_watchlist_at": "2035-01-05T00:00:00Z",
      "status": "watching",
      "watched_episodes_count": 3,
      "total_episodes_count": 12,
      "show": {
        "title": "Fixture Anime Season Two",
        "poster": "00/0000fixture2",
        "year": 2033,
        "runtime": 24,
        "ids": {
          "simkl": 9900201,
          "slug": "fixture-anime-season-two",
          "mal": "990201",
          "anilist": 990201,
          "anidb": "990211",
          "kitsu": "990221",
          "tvdb": "880201",
          "tmdb": "770201",
          "imdb": "tt9900201"
        }
      },
      "anime_type": "tv",
      "mapped_tvdb_seasons": [2],
      "seasons": [
        {
          "number": 1,
          "episodes": [
            { "number": 1, "tvdb": { "season": 2, "episode": 1 } }
          ]
        }
      ]
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Season One",
        "year": 2032,
        "ids": { "simkl": 9900202, "mal": "990202", "tvdb": "880201" }
      },
      "anime_type": "tv",
      "mapped_tvdb_seasons": [1, 1]
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Split Cour",
        "year": 2033,
        "ids": { "simkl": 9900203, "tvdb": "880203" }
      },
      "anime_type": "ona",
      "mapped_tvdb_seasons": [1, 2]
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Specials",
        "ids": { "simkl": 9900204, "tvdb": "880201" }
      },
      "anime_type": "special",
      "mapped_tvdb_seasons": [0]
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Feature",
        "year": 2034,
        "ids": { "simkl": 9900205, "tmdb": "770205", "mal": "990205" }
      },
      "anime_type": "movie",
      "mapped_tvdb_seasons": [1]
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Music Clip",
        "ids": { "simkl": 9900206 }
      },
      "anime_type": "music video"
    },
    {
      "status": "watching",
      "show": {
        "title": "Fixture Anime Unmapped",
        "ids": { "simkl": 9900207, "mal": 990207 }
      },
      "anime_type": null
    }
  ]
}"#;

const MOVIES: &str = r#"{
  "movies": [
    {
      "added_to_watchlist_at": "2035-01-03T00:00:00Z",
      "last_watched_at": null,
      "user_rated_at": null,
      "user_rating": null,
      "status": "plantowatch",
      "movie": {
        "title": "Fixture Feature One",
        "poster": "00/0000fixture3",
        "year": 2030,
        "ids": {
          "simkl": 9900301,
          "slug": "fixture-feature-one",
          "imdb": "tt9900301",
          "tmdb": "770301"
        }
      }
    },
    {
      "status": "plantowatch",
      "movie": {
        "title": "Fixture Feature Two",
        "year": 2031,
        "ids": { "simkl": 9900302, "tmdb": 770302, "tvdb": "880302" }
      }
    }
  ]
}"#;

const SETTINGS: &str = r#"{
  "user": {
    "name": "fixture_member",
    "joined_at": "2030-06-12T14:23:08.000Z",
    "gender": "",
    "avatar": "https://simkl.in/avatars/00/0000fixture/user_100.jpg",
    "bio": "",
    "loc": null,
    "age": ""
  },
  "account": {
    "id": 990001,
    "timezone": "UTC",
    "type": "free",
    "anime_title_language": "en"
  }
}"#;

fn api(path: &str) -> String {
    format!(
        "https://api.simkl.com{path}?client_id={CLIENT_ID}&app-name=scryer&app-version={}",
        env!("CARGO_PKG_VERSION")
    )
}

fn activities_url() -> String {
    api("/sync/activities")
}

fn items_url(library: &str, status: &str) -> String {
    let url = api(&format!("/sync/all-items/{library}/{status}"));
    if library == "anime" {
        format!("{url}&extended=full_anime_seasons")
    } else {
        url
    }
}

fn credential() -> ListCredential {
    ListCredential {
        access_token: TOKEN.to_string(),
        token_type: Some("bearer".to_string()),
        external_user_id: Some("990001".to_string()),
        username: Some("fixture_member".to_string()),
    }
}

fn request(status: &str, kind: Option<&str>, since: Option<&str>) -> ListPluginFetchRequest {
    ListPluginFetchRequest {
        source_type: status.to_string(),
        params: kind
            .map(|kind| BTreeMap::from([(PARAM_TYPE.to_string(), kind.to_string())]))
            .unwrap_or_default(),
        credential: Some(credential()),
        page_cursor: None,
        since_fingerprint: since.map(str::to_string),
    }
}

fn library_http(status: &str, activities: &str) -> RecordedHttp {
    RecordedHttp::new()
        .with(&activities_url(), 200, activities)
        .with(&items_url("shows", status), 200, SHOWS)
        .with(&items_url("anime", status), 200, ANIME)
        .with(&items_url("movies", status), 200, MOVIES)
}

fn fetch_ok(http: &RecordedHttp, request: &ListPluginFetchRequest) -> ListPluginFetchResponse {
    block_on(fetch(http, CLIENT_ID, request)).unwrap()
}

fn fetch_err(http: &RecordedHttp, request: &ListPluginFetchRequest) -> PluginError {
    block_on(fetch(http, CLIENT_ID, request)).unwrap_err()
}

fn ids_of(item: &ListPluginItem) -> Vec<(&str, &str, Option<&str>)> {
    item.external_ids
        .iter()
        .map(|id| (id.source.as_str(), id.id.as_str(), id.kind.as_deref()))
        .collect()
}

fn find<'a>(items: &'a [ListPluginItem], key: &str) -> &'a ListPluginItem {
    items
        .iter()
        .find(|item| item.item_key == key)
        .unwrap_or_else(|| panic!("no item {key}"))
}

fn list_descriptor() -> scryer_plugin_sdk::ListProviderDescriptor {
    descriptor().list_provider().cloned().unwrap()
}

#[test]
fn descriptor_round_trips_and_passes_host_checks() {
    let original = descriptor();
    let bytes = serde_json::to_vec(&original).unwrap();
    let decoded: PluginDescriptor = serde_json::from_slice(&bytes).unwrap();
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

    let list = list_descriptor();
    assert_eq!(decoded.id, "simkl-list");
    assert_eq!(list.provider_type, "simkl");
    assert_eq!(
        list.auth,
        ListProviderAuth::MemberAccount {
            flow: ListAccountFlow::AuthorizationCode { pkce: true },
            exchange: ListAccountExchange::SmgRelay,
            byo_app: false,
            scopes: vec!["media:read".to_string()],
        }
    );
    assert!(list.capabilities.account);
    assert!(!list.capabilities.health);
    assert!(list.capabilities.requires_member_credential);
    assert_eq!(list.allowed_hosts, vec!["api.simkl.com".to_string()]);
    assert_eq!(list.rate_limit_seconds, Some(2));
    assert!(
        list.config_fields.is_empty(),
        "no credential may live in plugin config"
    );
    assert!(list.url_patterns.is_empty(), "nothing here is public");
    assert_eq!(
        list.coverage,
        vec![
            ListMediaKind::Movie,
            ListMediaKind::Series,
            ListMediaKind::Anime
        ]
    );
    let notes: Vec<_> = list
        .notes
        .iter()
        .map(|note| note.text_key.as_str())
        .collect();
    assert_eq!(notes, vec!["lists.note.simkl_anime_seasons"]);
}

#[test]
fn every_status_is_a_personal_six_hour_source() {
    let list = list_descriptor();
    assert_eq!(list.groups.len(), 1);
    assert_eq!(list.groups[0].auth_badge, ListAuthBadge::MemberAccount);
    let items = &list.groups[0].items;
    let sources: Vec<_> = items.iter().map(|item| item.source_type.as_str()).collect();
    assert_eq!(
        sources,
        vec!["watching", "plantowatch", "hold", "completed", "dropped"]
    );
    let all = vec![
        ListMediaKind::Movie,
        ListMediaKind::Series,
        ListMediaKind::Anime,
    ];
    let shows_only = vec![ListMediaKind::Series, ListMediaKind::Anime];
    for item in items {
        assert!(item.personal, "{}", item.id);
        assert_eq!(item.default_interval_seconds, 6 * 60 * 60, "{}", item.id);
        let status = Status::parse(&item.source_type).unwrap();
        let movies = !matches!(status, Status::Watching | Status::OnHold);
        assert_eq!(
            item.kinds,
            if movies {
                all.clone()
            } else {
                shows_only.clone()
            }
        );
        assert_eq!(item.params.len(), 1);
        let param = &item.params[0];
        assert_eq!(param.key, "type");
        assert_eq!(param.param_type, ListSourceParamType::Enum);
        assert!(!param.required);
        let expected: &[&str] = if movies {
            &["all", "movies", "shows", "anime"]
        } else {
            &["all", "shows", "anime"]
        };
        assert_eq!(param.options, expected, "{}", item.id);
    }
}

#[test]
fn each_status_and_type_reads_only_its_libraries() {
    for status in Status::ALL {
        let key = status.key();
        let movies = !matches!(status, Status::Watching | Status::OnHold);
        let mut cases: Vec<(Option<&str>, Vec<&str>)> = vec![
            (
                None,
                if movies {
                    vec!["shows", "anime", "movies"]
                } else {
                    vec!["shows", "anime"]
                },
            ),
            (
                Some("all"),
                if movies {
                    vec!["shows", "anime", "movies"]
                } else {
                    vec!["shows", "anime"]
                },
            ),
            (Some("shows"), vec!["shows"]),
            (Some("anime"), vec!["anime"]),
            (Some(" Anime "), vec!["anime"]),
        ];
        if movies {
            cases.push((Some("movies"), vec!["movies"]));
        }
        for (kind, libraries) in cases {
            let http = library_http(key, ACTIVITIES);
            let response = fetch_ok(&http, &request(key, kind, None));
            let mut expected = vec![activities_url()];
            expected.extend(libraries.iter().map(|library| items_url(library, key)));
            assert_eq!(http.urls(), expected, "{key} {kind:?}");
            assert!(!response.unchanged);
            assert!(response.next_cursor.is_none());
            assert_eq!(response.list_name.as_deref(), Some(status.label()));
            assert_eq!(response.total_hint, Some(response.items.len() as u32));

            let hints: BTreeSet<_> = response
                .items
                .iter()
                .map(|item| format!("{:?}", item.kind_hint.unwrap()))
                .collect();
            let mut want = BTreeSet::new();
            for library in &libraries {
                match *library {
                    "shows" => {
                        want.insert("Series".to_string());
                    }
                    "anime" => {
                        want.insert("Anime".to_string());
                        want.insert("Movie".to_string());
                    }
                    _ => {
                        want.insert("Movie".to_string());
                    }
                }
            }
            assert_eq!(hints, want, "{key} {kind:?}");
            let ranks: Vec<_> = response.items.iter().map(|item| item.rank).collect();
            let expected_ranks: Vec<_> = (1..=response.items.len() as u32).map(Some).collect();
            assert_eq!(ranks, expected_ranks);
        }
    }
}

#[test]
fn movies_are_refused_for_statuses_simkl_does_not_have() {
    for status in [SOURCE_WATCHING, SOURCE_ON_HOLD] {
        let http = library_http(status, ACTIVITIES);
        let error = fetch_err(&http, &request(status, Some("movies"), None));
        assert_eq!(error.code, PluginErrorCode::InvalidConfig, "{status}");
        assert!(http.urls().is_empty());
    }
    let http = library_http(SOURCE_COMPLETED, ACTIVITIES);
    let error = fetch_err(&http, &request(SOURCE_COMPLETED, Some("episodes"), None));
    assert_eq!(error.code, PluginErrorCode::InvalidConfig);
    assert!(http.urls().is_empty());
}

#[test]
fn shows_carry_their_ids_and_a_simkl_key() {
    let http = library_http(SOURCE_WATCHING, ACTIVITIES);
    let response = fetch_ok(&http, &request(SOURCE_WATCHING, Some("shows"), None));
    assert_eq!(response.items.len(), 2, "the repeated entry is dropped");

    let show = find(&response.items, "simkl:show:9900101");
    assert_eq!(show.rank, Some(1));
    assert_eq!(show.kind_hint, Some(ListMediaKind::Series));
    assert_eq!(show.title.as_deref(), Some("Fixture Serial One"));
    assert_eq!(show.year, Some(2031));
    assert_eq!(show.season, None);
    assert_eq!(show.format, None);
    assert_eq!(
        ids_of(show),
        vec![
            ("tmdb", "770101", Some("series")),
            ("imdb", "tt9900101", Some("series")),
            ("tvdb", "880101", Some("series")),
            ("simkl", "9900101", Some("series")),
        ]
    );

    // Simkl always sends its own id; without one the strongest other id keys
    // the entry.
    let fallback = find(&response.items, "tvdb:series:880102");
    assert_eq!(fallback.rank, Some(2));
    assert_eq!(ids_of(fallback), vec![("tvdb", "880102", Some("series"))]);
}

#[test]
fn anime_seasons_stay_apart_and_carry_their_tvdb_season() {
    let http = library_http(SOURCE_WATCHING, ACTIVITIES);
    let response = fetch_ok(&http, &request(SOURCE_WATCHING, Some("anime"), None));
    let keys: Vec<_> = response
        .items
        .iter()
        .map(|item| item.item_key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec![
            "simkl:anime:9900201",
            "simkl:anime:9900202",
            "simkl:anime:9900203",
            "simkl:anime:9900204",
            "simkl:anime:9900205",
            "simkl:anime:9900207",
        ],
        "every season is its own item and music videos are skipped"
    );

    let season_two = find(&response.items, "simkl:anime:9900201");
    assert_eq!(season_two.kind_hint, Some(ListMediaKind::Anime));
    assert_eq!(season_two.season, Some(2));
    assert_eq!(season_two.format.as_deref(), Some("tv"));
    assert_eq!(
        season_two.title.as_deref(),
        Some("Fixture Anime Season Two")
    );
    assert_eq!(season_two.year, Some(2033));
    assert_eq!(
        ids_of(season_two),
        vec![
            ("tmdb", "770201", Some("series")),
            ("imdb", "tt9900201", Some("series")),
            ("tvdb", "880201", Some("series")),
            ("simkl", "9900201", Some("anime")),
            ("mal", "990201", Some("anime")),
            ("anilist", "990201", Some("anime")),
            ("anidb", "990211", Some("anime")),
            ("kitsu", "990221", Some("anime")),
        ]
    );

    let season_one = find(&response.items, "simkl:anime:9900202");
    assert_eq!(season_one.season, Some(1), "repeats of one season are one");
    assert_eq!(
        ids_of(season_one),
        vec![
            ("tvdb", "880201", Some("series")),
            ("simkl", "9900202", Some("anime")),
            ("mal", "990202", Some("anime")),
        ]
    );

    let split = find(&response.items, "simkl:anime:9900203");
    assert_eq!(
        split.season, None,
        "an entry over two seasons is the series"
    );
    assert_eq!(split.format.as_deref(), Some("ona"));

    let specials = find(&response.items, "simkl:anime:9900204");
    assert_eq!(specials.season, Some(0));
    assert_eq!(specials.format.as_deref(), Some("special"));
    assert_eq!(specials.year, None);

    let feature = find(&response.items, "simkl:anime:9900205");
    assert_eq!(feature.kind_hint, Some(ListMediaKind::Movie));
    assert_eq!(feature.season, None, "a movie has no season");
    assert_eq!(feature.format.as_deref(), Some("movie"));
    assert_eq!(
        ids_of(feature),
        vec![
            ("tmdb", "770205", Some("movie")),
            ("simkl", "9900205", Some("anime")),
            ("mal", "990205", Some("anime")),
        ]
    );

    let unmapped = find(&response.items, "simkl:anime:9900207");
    assert_eq!(unmapped.kind_hint, Some(ListMediaKind::Anime));
    assert_eq!(unmapped.season, None);
    assert_eq!(unmapped.format, None);
}

#[test]
fn season_mapping_rejects_anything_but_whole_seasons() {
    let season = |value: &str| mapped_season(&serde_json::from_str(value).unwrap());
    assert_eq!(season(r#"{"mapped_tvdb_seasons": [3]}"#), Some(3));
    assert_eq!(season(r#"{"mapped_tvdb_seasons": [0, 0]}"#), Some(0));
    assert_eq!(season(r#"{"mapped_tvdb_seasons": []}"#), None);
    assert_eq!(season(r#"{"mapped_tvdb_seasons": [1, 2]}"#), None);
    assert_eq!(season(r#"{"mapped_tvdb_seasons": [-1]}"#), None);
    assert_eq!(season(r#"{"mapped_tvdb_seasons": ["2"]}"#), None);
    assert_eq!(season(r#"{"mapped_tvdb_seasons": null}"#), None);
    assert_eq!(season(r#"{}"#), None);
}

#[test]
fn movies_map_to_movie_items() {
    let http = library_http(SOURCE_PLAN_TO_WATCH, ACTIVITIES);
    let response = fetch_ok(&http, &request(SOURCE_PLAN_TO_WATCH, Some("movies"), None));
    assert_eq!(response.items.len(), 2);

    let first = &response.items[0];
    assert_eq!(first.item_key, "simkl:movie:9900301");
    assert_eq!(first.kind_hint, Some(ListMediaKind::Movie));
    assert_eq!(first.title.as_deref(), Some("Fixture Feature One"));
    assert_eq!(first.year, Some(2030));
    assert_eq!(
        ids_of(first),
        vec![
            ("tmdb", "770301", Some("movie")),
            ("imdb", "tt9900301", Some("movie")),
            ("simkl", "9900301", Some("movie")),
        ]
    );
    let second = &response.items[1];
    assert_eq!(
        ids_of(second),
        vec![
            ("tmdb", "770302", Some("movie")),
            ("tvdb", "880302", Some("movie")),
            ("simkl", "9900302", Some("movie")),
        ]
    );
}

#[test]
fn unchanged_activity_skips_the_library_read() {
    let http = library_http(SOURCE_WATCHING, ACTIVITIES);
    let first = fetch_ok(&http, &request(SOURCE_WATCHING, None, None));
    let fingerprint = first.fingerprint.clone().unwrap();
    assert!(fingerprint.starts_with("simkl:v1:"));
    assert!(!fingerprint.contains(TOKEN));

    let again = library_http(SOURCE_WATCHING, ACTIVITIES);
    let second = fetch_ok(&again, &request(SOURCE_WATCHING, None, Some(&fingerprint)));
    assert!(second.unchanged);
    assert!(second.items.is_empty());
    assert_eq!(second.fingerprint.as_deref(), Some(fingerprint.as_str()));
    assert_eq!(again.urls(), vec![activities_url()]);

    let moved = ACTIVITIES.replace("2035-02-02T10:00:00Z", "2035-02-09T10:00:00Z");
    let changed = library_http(SOURCE_WATCHING, &moved);
    let third = fetch_ok(
        &changed,
        &request(SOURCE_WATCHING, None, Some(&fingerprint)),
    );
    assert!(!third.unchanged);
    assert_eq!(third.items, first.items);
    assert_ne!(third.fingerprint.as_deref(), Some(fingerprint.as_str()));
    assert_eq!(changed.urls().len(), 3);
}

#[test]
fn the_fingerprint_follows_only_the_libraries_read() {
    let shows_only = fetch_ok(
        &library_http(SOURCE_WATCHING, ACTIVITIES),
        &request(SOURCE_WATCHING, Some("shows"), None),
    )
    .fingerprint
    .unwrap();

    // Anime activity does not touch a shows-only source.
    let anime_moved = ACTIVITIES.replace("2035-02-02T10:00:00Z", "2035-02-09T10:00:00Z");
    let http = library_http(SOURCE_WATCHING, &anime_moved);
    let response = fetch_ok(
        &http,
        &request(SOURCE_WATCHING, Some("shows"), Some(&shows_only)),
    );
    assert!(response.unchanged);

    // Another status moving in the same library does not touch it either.
    let other_status = ACTIVITIES.replace(
        r#""plantowatch": "2035-02-01T10:00:00Z""#,
        r#""plantowatch": "2035-02-09T10:00:00Z""#,
    );
    let http = library_http(SOURCE_WATCHING, &other_status);
    let response = fetch_ok(
        &http,
        &request(SOURCE_WATCHING, Some("shows"), Some(&shows_only)),
    );
    assert!(response.unchanged);
    assert_eq!(http.urls(), vec![activities_url()]);

    // Its own status moving, or an item leaving the library, rereads it.
    for moved in [
        ACTIVITIES.replace(
            r#""watching": "2035-01-20T10:00:00Z""#,
            r#""watching": "2035-02-09T10:00:00Z""#,
        ),
        ACTIVITIES.replacen(
            r#""removed_from_list": null"#,
            r#""removed_from_list": "2035-02-09T10:00:00Z""#,
            1,
        ),
    ] {
        let http = library_http(SOURCE_WATCHING, &moved);
        let response = fetch_ok(
            &http,
            &request(SOURCE_WATCHING, Some("shows"), Some(&shows_only)),
        );
        assert!(!response.unchanged);
        assert_eq!(response.items.len(), 2);
        assert_eq!(http.urls().len(), 2);
    }

    // The same timestamps under another status are another fingerprint.
    let completed = fetch_ok(
        &library_http(SOURCE_COMPLETED, ACTIVITIES),
        &request(SOURCE_COMPLETED, Some("shows"), None),
    )
    .fingerprint
    .unwrap();
    assert_ne!(completed, shows_only);
}

#[test]
fn a_new_member_without_activity_is_still_fingerprinted() {
    // Simkl's shape for a member who has not touched the library yet.
    let fresh = r#"{
      "all": null,
      "settings": { "all": null },
      "tv_shows": {
        "all": null, "rated_at": null, "playback": null, "plantowatch": null,
        "watching": null, "completed": null, "hold": null, "dropped": null,
        "removed_from_list": null
      },
      "anime": {
        "all": null, "rated_at": null, "playback": null, "plantowatch": null,
        "watching": null, "completed": null, "hold": null, "dropped": null,
        "removed_from_list": null
      },
      "movies": {
        "all": null, "rated_at": null, "playback": null, "plantowatch": null,
        "completed": null, "dropped": null, "removed_from_list": null
      },
      "custom_lists": { "lists": { "all": null } }
    }"#;
    let empty = RecordedHttp::new()
        .with(&activities_url(), 200, fresh)
        .with(&items_url("shows", SOURCE_DROPPED), 200, "{}")
        .with(&items_url("anime", SOURCE_DROPPED), 200, "{}")
        .with(&items_url("movies", SOURCE_DROPPED), 200, "{}");
    let first = fetch_ok(&empty, &request(SOURCE_DROPPED, None, None));
    assert!(first.items.is_empty());
    let fingerprint = first.fingerprint.unwrap();
    assert!(fingerprint.starts_with("simkl:v1:"));

    let again = RecordedHttp::new().with(&activities_url(), 200, fresh);
    let second = fetch_ok(&again, &request(SOURCE_DROPPED, None, Some(&fingerprint)));
    assert!(second.unchanged);
    assert_eq!(again.urls(), vec![activities_url()]);
}

#[test]
fn unusable_activity_falls_back_to_a_content_fingerprint() {
    for activities in [
        r#"{"tv_shows": {"watching": "2035-01-20T10:00:00Z"}}"#,
        r#"{"tv_shows": {"all": "2035-01-20T10:00:00Z", "removed_from_list": null}}"#,
        r#"{"tv_shows": {"watching": 7, "removed_from_list": null}}"#,
        r#"{}"#,
        "",
        "null",
    ] {
        let http = RecordedHttp::new()
            .with(&activities_url(), 200, activities)
            .with(&items_url("shows", SOURCE_WATCHING), 200, SHOWS);
        let first = fetch_ok(&http, &request(SOURCE_WATCHING, Some("shows"), None));
        assert_eq!(first.items.len(), 2, "{activities}");
        let fingerprint = first.fingerprint.unwrap();
        assert!(!fingerprint.starts_with("simkl:"), "{activities}");

        let again = RecordedHttp::new()
            .with(&activities_url(), 200, activities)
            .with(&items_url("shows", SOURCE_WATCHING), 200, SHOWS);
        let second = fetch_ok(
            &again,
            &request(SOURCE_WATCHING, Some("shows"), Some(&fingerprint)),
        );
        assert!(second.unchanged, "{activities}");
        assert_eq!(again.urls().len(), 2, "the library is always read");
    }
}

#[test]
fn empty_libraries_are_empty_lists() {
    for body in [
        "{}",
        "",
        "  ",
        "null",
        "[]",
        r#"{"shows": null}"#,
        r#"{"movies": []}"#,
    ] {
        let http = RecordedHttp::new()
            .with(&activities_url(), 200, ACTIVITIES)
            .with(&items_url("shows", SOURCE_COMPLETED), 200, body);
        let response = fetch_ok(&http, &request(SOURCE_COMPLETED, Some("shows"), None));
        assert!(response.items.is_empty(), "{body:?}");
        assert!(!response.unchanged);
    }
}

#[test]
fn credential_less_fetches_are_refused_before_any_request() {
    let http = library_http(SOURCE_WATCHING, ACTIVITIES);
    let missing = ListPluginFetchRequest {
        credential: None,
        ..request(SOURCE_WATCHING, None, None)
    };
    let error = fetch_err(&http, &missing);
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
    assert!(error.public_message.contains("linked Simkl account"));

    let blank = ListPluginFetchRequest {
        credential: Some(ListCredential {
            access_token: "  ".to_string(),
            ..credential()
        }),
        ..request(SOURCE_WATCHING, None, None)
    };
    assert_eq!(fetch_err(&http, &blank).code, PluginErrorCode::AuthFailed);
    assert!(http.urls().is_empty());
}

#[test]
fn an_unregistered_app_refuses_every_call_before_any_request() {
    let http = library_http(SOURCE_WATCHING, ACTIVITIES);
    for client_id in ["", "   "] {
        match block_on(run(
            &http,
            client_id,
            PluginListCommand::Fetch(request(SOURCE_WATCHING, None, None)),
        )) {
            PluginListCommandResult::Fetch(PluginResult::Err(error)) => {
                assert_eq!(error.code, PluginErrorCode::InvalidConfig);
                assert!(error.public_message.contains("no Simkl app id"));
            }
            other => panic!("unexpected {other:?}"),
        }
        match block_on(run(
            &http,
            client_id,
            PluginListCommand::Account(ListPluginAccountRequest {
                credential: credential(),
            }),
        )) {
            PluginListCommandResult::Account(PluginResult::Err(error)) => {
                assert_eq!(error.code, PluginErrorCode::InvalidConfig)
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(http.urls().is_empty());
}

#[test]
fn requests_name_the_app_and_carry_the_token_only_in_a_header() {
    let http = library_http(SOURCE_PLAN_TO_WATCH, ACTIVITIES);
    fetch_ok(&http, &request(SOURCE_PLAN_TO_WATCH, None, None));
    let requests = http.requests();
    assert_eq!(requests.len(), 4);
    for request in requests {
        assert_eq!(request.method.as_deref(), Some("GET"));
        assert!(request.body.is_empty());
        assert!(!request.url.contains(TOKEN), "{}", request.url);
        assert!(request.url.starts_with("https://api.simkl.com/"));
        assert!(request.url.contains("client_id=fixture-client-id"));
        assert!(request.url.contains("app-name=scryer"));
        assert_eq!(
            request.headers.get("Authorization").map(String::as_str),
            Some(format!("Bearer {TOKEN}").as_str())
        );
        assert_eq!(
            request.headers.get("User-Agent").map(String::as_str),
            Some(concat!("simkl-list-provider/", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            request.headers.get("Accept").map(String::as_str),
            Some("application/json")
        );
        // The client id goes once, in the URL; Content-Type is for writes.
        assert!(!request.headers.contains_key("simkl-api-key"));
        assert!(!request.headers.contains_key("Content-Type"));
    }
}

#[test]
fn upstream_failures_map_to_host_classes() {
    let cases = [
        (401, PluginErrorCode::AuthFailed, false),
        // A 403 Simkl gives no reason for is a refusal retrying cannot fix.
        (403, PluginErrorCode::Permanent, false),
        (404, PluginErrorCode::Permanent, true),
        (412, PluginErrorCode::UpstreamUnavailable, false),
        (429, PluginErrorCode::RateLimited, false),
        (500, PluginErrorCode::UpstreamUnavailable, false),
        (502, PluginErrorCode::UpstreamUnavailable, false),
        (400, PluginErrorCode::Permanent, false),
    ];
    for (status, code, not_found) in cases {
        // A failure on the activity read stops the sync before the library.
        let http = RecordedHttp::new()
            .with_headers(&activities_url(), status, &[("Retry-After", "120")], "")
            .with(&items_url("shows", SOURCE_WATCHING), 200, SHOWS);
        let error = fetch_err(&http, &request(SOURCE_WATCHING, Some("shows"), None));
        assert_eq!(error.code, code, "{status}");
        assert_eq!(
            error.public_message.contains("not found"),
            not_found,
            "{status}"
        );
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(120));
        }
        assert_eq!(http.urls(), vec![activities_url()], "{status}");

        // So does a failure on the library read itself.
        let http = RecordedHttp::new()
            .with(&activities_url(), 200, ACTIVITIES)
            .with_headers(
                &items_url("anime", SOURCE_WATCHING),
                status,
                &[("Retry-After", "120")],
                r#"{"error":"fixture_error","code":0,"message":"Fixture failure"}"#,
            );
        let error = fetch_err(&http, &request(SOURCE_WATCHING, Some("anime"), None));
        assert_eq!(error.code, code, "{status}");
    }
}

#[test]
fn rejected_tokens_name_simkls_reason_but_never_the_token() {
    let named = RecordedHttp::new().with(
        &activities_url(),
        401,
        r#"{"error":"invalid_token","code":401,"message":"Token expired"}"#,
    );
    let error = fetch_err(&named, &request(SOURCE_WATCHING, None, None));
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
    assert!(error.public_message.contains("HTTP 401, invalid_token"));

    let echoed = RecordedHttp::new().with(
        &activities_url(),
        401,
        &format!(r#"{{"error":"{TOKEN}","message":"bad token {TOKEN}"}}"#),
    );
    let error = fetch_err(&echoed, &request(SOURCE_WATCHING, None, None));
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
    assert!(!error.public_message.contains(TOKEN));
    assert!(!format!("{error:?}").contains(TOKEN));

    let refused = RecordedHttp::new().with(
        &activities_url(),
        412,
        r#"{"error":"client_id_failed","code":412}"#,
    );
    let error = fetch_err(&refused, &request(SOURCE_WATCHING, None, None));
    assert_eq!(error.code, PluginErrorCode::UpstreamUnavailable);
    assert!(error.public_message.contains("client_id_failed"));
    assert!(!error.public_message.contains(CLIENT_ID));
}

#[test]
fn a_403_that_only_a_new_link_fixes_asks_for_one() {
    for name in ["insufficient_scope", "oauth2_token_required"] {
        let http = RecordedHttp::new().with(
            &activities_url(),
            403,
            &format!(r#"{{"error":"{name}","code":403,"message":"Fixture refusal"}}"#),
        );
        let error = fetch_err(&http, &request(SOURCE_WATCHING, None, None));
        assert_eq!(error.code, PluginErrorCode::AuthFailed, "{name}");
        assert!(error.public_message.contains(name), "{name}");
    }
    for name in ["forbidden", "private_list"] {
        let http = RecordedHttp::new().with(
            &activities_url(),
            403,
            &format!(r#"{{"error":"{name}","code":403}}"#),
        );
        let error = fetch_err(&http, &request(SOURCE_WATCHING, None, None));
        assert_eq!(error.code, PluginErrorCode::Permanent, "{name}");
        assert!(!error.public_message.contains("not found"), "{name}");
    }
}

#[test]
fn malformed_library_responses_are_permanent_failures() {
    for body in [
        "<html>maintenance</html>",
        r#"{"shows": {"title": "Fixture"}}"#,
        r#"[{"show": {}}]"#,
        r#""text""#,
        r#"{"error": "fixture_error"}"#,
    ] {
        let http = RecordedHttp::new()
            .with(&activities_url(), 200, ACTIVITIES)
            .with(&items_url("shows", SOURCE_WATCHING), 200, body);
        let error = fetch_err(&http, &request(SOURCE_WATCHING, Some("shows"), None));
        assert_eq!(error.code, PluginErrorCode::Permanent, "{body}");
        assert!(!error.public_message.contains("not found"), "{body}");
    }
}

#[test]
fn account_reads_the_identity_and_offers_every_status() {
    let http = RecordedHttp::new().with(&api("/users/settings"), 200, SETTINGS);
    let account = match block_on(run(
        &http,
        CLIENT_ID,
        PluginListCommand::Account(ListPluginAccountRequest {
            credential: credential(),
        }),
    )) {
        PluginListCommandResult::Account(PluginResult::Ok(account)) => account,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(http.urls(), vec![api("/users/settings")]);
    assert_eq!(
        http.requests()[0]
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(account.external_user_id, "990001");
    assert_eq!(account.username, "fixture_member");
    assert_eq!(account.display_name.as_deref(), Some("fixture_member"));
    assert_eq!(
        account.avatar_url.as_deref(),
        Some("https://simkl.in/avatars/00/0000fixture/user_100.jpg")
    );
    assert!(account.owned_lists.is_empty());
    let statuses: Vec<_> = account
        .statuses
        .iter()
        .map(|status| {
            (
                status.key.as_str(),
                status.label.as_str(),
                status.kinds.len(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("watching", "Watching", 2),
            ("plantowatch", "Plan to watch", 3),
            ("hold", "On hold", 2),
            ("completed", "Completed", 3),
            ("dropped", "Dropped", 3),
        ]
    );
    // Every status key is a source the descriptor declares, with the same
    // kinds.
    let list = list_descriptor();
    for status in &account.statuses {
        let item = list.groups[0]
            .items
            .iter()
            .find(|item| item.source_type == status.key)
            .unwrap();
        assert_eq!(item.kinds, status.kinds);
    }
}

#[test]
fn account_tolerates_a_private_profile_but_needs_an_id() {
    let private =
        r#"{"user": {"name": "", "avatar": "/img/default.png"}, "account": {"id": "990002"}}"#;
    let http = RecordedHttp::new().with(&api("/users/settings"), 200, private);
    let profile = block_on(account(&http, CLIENT_ID, &credential())).unwrap();
    assert_eq!(profile.external_user_id, "990002");
    assert_eq!(profile.username, "990002");
    assert_eq!(profile.display_name, None);
    assert_eq!(profile.avatar_url, None);

    let anonymous = RecordedHttp::new().with(&api("/users/settings"), 200, r#"{"user": {}}"#);
    let error = block_on(account(&anonymous, CLIENT_ID, &credential())).unwrap_err();
    assert_eq!(error.code, PluginErrorCode::Permanent);

    let revoked = RecordedHttp::new().with(
        &api("/users/settings"),
        401,
        r#"{"error":"user_token_failed"}"#,
    );
    let error = block_on(account(&revoked, CLIENT_ID, &credential())).unwrap_err();
    assert_eq!(error.code, PluginErrorCode::AuthFailed);
}

#[test]
fn unknown_sources_and_health_are_unsupported() {
    let http = RecordedHttp::new();
    let other = request("friends", None, None);
    assert_eq!(fetch_err(&http, &other).code, PluginErrorCode::Unsupported);
    match block_on(run(
        &http,
        CLIENT_ID,
        PluginListCommand::Health(ListPluginHealthRequest {}),
    )) {
        PluginListCommandResult::Health(PluginResult::Err(error)) => {
            assert_eq!(error.code, PluginErrorCode::Unsupported)
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(http.urls().is_empty());
}
