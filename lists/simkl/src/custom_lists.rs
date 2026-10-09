//! Authenticated custom-list discovery and bounded, complete list pagination.

use scryer_plugin_sdk::ListAccountList;

use super::*;

const PAGE_LIMIT: u32 = 500;
const MAX_ITEMS: u32 = 10_000;
const MAX_PAGES: u32 = 20;
// The cursor binds later pages to the same list and metadata snapshot.
type Cursor = (u32, String, String, u32, u32, String);

pub(super) fn source_item() -> ListProviderItem {
    ListProviderItem {
        id: "custom-list".into(),
        name: "Custom list".into(),
        description: Some("Your Simkl custom list (requires PRO or VIP)".into()),
        kinds: vec![
            ListMediaKind::Movie,
            ListMediaKind::Series,
            ListMediaKind::Anime,
        ],
        source_type: SOURCE_LIST.into(),
        params: vec![ListSourceParam {
            key: PARAM_LIST_ID.into(),
            label: "List ID".into(),
            param_type: ListSourceParamType::Text,
            options: Vec::new(),
            required: true,
        }],
        personal: true,
        default_interval_seconds: DEFAULT_INTERVAL_SECONDS,
    }
}

fn malformed() -> PluginError {
    permanent(
        "Simkl returned incomplete or inconsistent custom-list data; no list changes were applied",
    )
}

fn numeric_id(value: Option<&Value>) -> Result<String, PluginError> {
    let id = json_id(value).ok_or_else(malformed)?;
    if !id.bytes().all(|byte| byte.is_ascii_digit())
        || id.parse::<u64>().ok().filter(|id| *id > 0).is_none()
    {
        return Err(malformed());
    }
    Ok(id)
}

fn check_body(body: &Value) -> Result<(), PluginError> {
    if body.get("error").is_some() {
        return Err(match error_name(body) {
            Some("premium_only") => permanent("Simkl custom lists require a PRO or VIP account"),
            Some("oauth2_token_required" | "insufficient_scope") => auth_failed(
                "Simkl custom lists require AUTH V2 with media:read; reconnect the account",
            ),
            _ => permanent("Simkl returned an error instead of a custom list"),
        });
    }
    Ok(())
}

fn kinds(media: &str) -> Result<Vec<ListMediaKind>, PluginError> {
    match media {
        "movies" => Ok(vec![ListMediaKind::Movie]),
        "tv" => Ok(vec![ListMediaKind::Series]),
        // A Simkl anime list may include both series and movies.
        "anime" => Ok(vec![ListMediaKind::Anime, ListMediaKind::Movie]),
        _ => Err(malformed()),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Page {
    limit: u32,
    total: u32,
    next: Option<u32>,
}

fn page<'a>(
    body: &'a Value,
    key: &str,
    requested: u32,
) -> Result<(&'a [Value], Page), PluginError> {
    check_body(body)?;
    let entries = body
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    let pagination = body.get("pagination").ok_or_else(malformed)?;
    let number = |key| {
        pagination
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(malformed)
    };
    let actual = number("page")?;
    let limit = number("limit")?;
    let total = number("total_items")?;
    let pages = number("total_pages")?;
    if requested == 0
        || actual != requested
        || limit == 0
        || limit > PAGE_LIMIT
        || total > MAX_ITEMS
        || pages > MAX_PAGES
        || actual.checked_mul(limit).is_none_or(|end| end > MAX_ITEMS)
        || (total > 0 && (pages != total.div_ceil(limit) || actual > pages))
        || (total == 0 && (actual != 1 || pages > 1))
    {
        return Err(malformed());
    }
    let offset = (actual - 1) * limit;
    if entries.len() != total.saturating_sub(offset).min(limit) as usize {
        return Err(malformed());
    }
    Ok((
        entries,
        Page {
            limit,
            total,
            next: (actual < pages).then_some(actual + 1),
        },
    ))
}

impl<H: ListHttp> Client<'_, H> {
    pub(super) async fn custom_lists(
        &self,
        user_id: &str,
    ) -> Result<Vec<ListAccountList>, PluginError> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut requested = 1;
        let mut expected = None;
        loop {
            let body = self
                .get_json(
                    &format!("/lists/user/{}", encode_component(user_id)),
                    &format!("limit={PAGE_LIMIT}&page={requested}"),
                    "Simkl custom lists",
                )
                .await?;
            // A free account still has a working watchlist and must remain linkable.
            // Fetching a custom list itself reports the subscription requirement.
            if requested == 1 && error_name(&body) == Some("premium_only") {
                return Ok(Vec::new());
            }
            let (entries, paging) = page(&body, "lists", requested)?;
            if expected.is_some_and(|prior| prior != (paging.total, paging.limit)) {
                return Err(malformed());
            }
            expected = Some((paging.total, paging.limit));
            for entry in entries {
                let id = numeric_id(entry.get("id"))?;
                if !seen.insert(id.clone()) {
                    return Err(malformed());
                }
                let name = json_text(entry.get("name")).ok_or_else(malformed)?;
                let media = entry
                    .get("media_type")
                    .and_then(Value::as_str)
                    .ok_or_else(malformed)?;
                result.push(ListAccountList {
                    id,
                    name,
                    kinds: kinds(media)?,
                });
            }
            match paging.next {
                Some(next) => requested = next,
                None => return Ok(result),
            }
        }
    }

    pub(super) async fn custom_list(
        &self,
        request: &ListPluginFetchRequest,
    ) -> Result<ListPluginFetchResponse, PluginError> {
        let raw_id = request
            .params
            .get(PARAM_LIST_ID)
            .map(|value| Value::String(value.trim().into()));
        let id = numeric_id(raw_id.as_ref())
            .map_err(|_| invalid_config("Simkl custom lists require a positive numeric list_id"))?;
        let cursor: Option<Cursor> = request
            .page_cursor
            .as_deref()
            .map(|cursor| {
                serde_json::from_str(cursor)
                    .map_err(|_| invalid_config("Invalid Simkl custom-list page cursor"))
            })
            .transpose()?;
        let requested = cursor.as_ref().map_or(1, |cursor| cursor.0);
        if requested == 0
            || requested > MAX_PAGES
            || cursor
                .as_ref()
                .is_some_and(|cursor| cursor.1 != id || cursor.0 < 2)
        {
            return Err(invalid_config("Invalid Simkl custom-list page cursor"));
        }
        let body = self
            .get_json(
                &format!("/lists/{id}"),
                &format!("limit={PAGE_LIMIT}&page={requested}"),
                "Simkl custom list",
            )
            .await?;
        let (entries, paging) = page(&body, "items", requested)?;
        if numeric_id(body.get("id"))? != id {
            return Err(malformed());
        }
        let media = body
            .get("media_type")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        kinds(media)?;
        let stamp = json_text(body.get("updated_at")).ok_or_else(malformed)?;
        if cursor.as_ref().is_some_and(|cursor| {
            cursor.2 != stamp
                || cursor.3 != paging.total
                || cursor.4 != paging.limit
                || cursor.5 != media
        }) {
            return Err(permanent(
                "Simkl custom list changed during pagination; retry the complete list",
            ));
        }
        let mut items = Vec::with_capacity(entries.len());
        let mut seen = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let mut item = custom_item(entry, media)?;
            if !seen.insert(item.item_key.clone()) {
                return Err(malformed());
            }
            // Preserve response order; leave sort/direction unset to honor the owner.
            item.rank = Some((requested - 1) * paging.limit + index as u32 + 1);
            items.push(item);
        }
        let next_cursor = paging
            .next
            .map(|next| {
                serde_json::to_string(&(next, &id, &stamp, paging.total, paging.limit, media))
            })
            .transpose()
            .map_err(|_| malformed())?;
        Ok(ListPluginFetchResponse {
            items,
            next_cursor,
            total_hint: Some(paging.total),
            list_name: json_text(body.get("name")),
            // No timestamp short-circuit: auto lists can change without updated_at moving.
            fingerprint: None,
            unchanged: false,
            ..Default::default()
        })
    }
}

fn custom_item(entry: &Value, media_type: &str) -> Result<ListPluginItem, PluginError> {
    let library = match (media_type, entry.get("type").and_then(Value::as_str)) {
        ("movies", Some("movie")) => Library::Movies,
        ("tv", Some("tv")) => Library::Shows,
        ("anime", Some("anime")) => Library::Anime,
        _ => return Err(malformed()),
    };
    let mut media = entry.clone();
    let simkl = numeric_id(media.get("ids").and_then(|ids| ids.get("simkl_id")))?;
    media
        .get_mut("ids")
        .and_then(Value::as_object_mut)
        .ok_or_else(malformed)?
        .insert("simkl".into(), Value::String(simkl.clone()));
    let mut anime_movies = BTreeSet::new();
    if matches!(library, Library::Anime) {
        let subtype = json_text(entry.get("anime_type")).ok_or_else(malformed)?;
        if subtype == "movie" {
            anime_movies.insert(simkl);
        }
    }
    let wrapped = serde_json::json!({"show":media});
    let mut item = entry_item(&wrapped, library, &anime_movies).ok_or_else(malformed)?;
    item.format = json_text(entry.get("anime_type"));
    item.language = json_text(entry.get("original_language"));
    Ok(item)
}
