use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use time::{format_description, OffsetDateTime};
use worker::{Fetch, Headers, Method, Request, RequestInit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapePlatform {
    TikTokKeyword,
    TikTokHashtag,
    InstagramReels,
}

impl TryFrom<(&str, &str)> for ScrapePlatform {
    type Error = ScrapeCreatorsError;

    fn try_from(value: (&str, &str)) -> Result<Self, Self::Error> {
        scrape_platform_from_str(value.0, value.1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedDiscoveryItem {
    pub external_id: String,
    pub platform: String,
    pub title: String,
    pub image_url: Option<String>,
    pub source_url: Option<String>,
    pub author_handle: String,
    pub like_count: Option<u64>,
    pub source_published_at: Option<String>,
}

#[derive(Debug, Error)]
pub enum ScrapeCreatorsError {
    #[error("unsupported scrape platform")]
    UnsupportedPlatform,
    #[error("scrapecreators endpoint returned status {status}")]
    HttpStatus {
        status: u16,
        raw_json: Option<Value>,
    },
    #[error("scrapecreators request failed: {0}")]
    Worker(#[from] worker::Error),
    #[error("failed to parse scrapecreators response: {0}")]
    Serde(#[from] serde_json::Error),
}

pub fn build_scrape_request(
    base_url: &str,
    platform: ScrapePlatform,
    query: &str,
    region: &str,
) -> Result<String, ScrapeCreatorsError> {
    let base = base_url.trim_end_matches('/');
    let encoded_query = url_encode(query);
    let encoded_region = url_encode(region);

    let url = match platform {
        ScrapePlatform::TikTokKeyword => format!(
            "{base}/v1/tiktok/search/keyword?query={encoded_query}&sort_by=date-posted&date_posted=last-6-months&trim=true&region={encoded_region}"
        ),
        ScrapePlatform::TikTokHashtag => format!(
            "{base}/v1/tiktok/search/hashtag?hashtag={encoded_query}&trim=true&region={encoded_region}"
        ),
        ScrapePlatform::InstagramReels => format!(
            "{base}/v2/instagram/reels/search?query={encoded_query}&date_posted=last-year"
        ),
    };

    Ok(url)
}

pub fn scrape_platform_from_str(
    platform: &str,
    search_kind: &str,
) -> Result<ScrapePlatform, ScrapeCreatorsError> {
    match (platform, search_kind) {
        ("tiktok", "keyword") => Ok(ScrapePlatform::TikTokKeyword),
        ("tiktok", "hashtag") => Ok(ScrapePlatform::TikTokHashtag),
        ("instagram", "reels") => Ok(ScrapePlatform::InstagramReels),
        _ => Err(ScrapeCreatorsError::UnsupportedPlatform),
    }
}

pub async fn fetch_scrapecreators_json(
    url: &str,
    api_key: &str,
) -> Result<Value, ScrapeCreatorsError> {
    let headers = Headers::new();
    headers.set("x-api-key", api_key)?;
    headers.set("accept", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);

    let request = Request::new_with_init(url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    let response_text = response.text().await.unwrap_or_default();
    let raw_json = serde_json::from_str::<Value>(&response_text).unwrap_or_else(|_| {
        json!({
            "rawText": response_text,
        })
    });

    if status >= 400 {
        return Err(ScrapeCreatorsError::HttpStatus {
            status,
            raw_json: Some(raw_json),
        });
    }

    Ok(raw_json)
}

pub fn normalize_tiktok_keyword_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    normalize_tiktok_items(raw)
}

pub fn normalize_tiktok_hashtag_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    normalize_tiktok_items(raw)
}

pub fn normalize_instagram_reels_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    array_at(raw, &["reels"])
        .or_else(|| array_at(raw, &["data"]))
        .or_else(|| array_at(raw, &["items"]))
        .into_iter()
        .flatten()
        .filter_map(normalize_instagram_reel)
        .collect()
}

fn normalize_tiktok_items(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    array_at(raw, &["search_item_list"])
        .or_else(|| array_at(raw, &["aweme_list"]))
        .or_else(|| array_at(raw, &["data"]))
        .or_else(|| array_at(raw, &["items"]))
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let aweme = item.get("aweme_info").unwrap_or(item);
            normalize_tiktok_aweme(aweme)
        })
        .collect()
}

fn normalize_tiktok_aweme(aweme: &Value) -> Option<NormalizedDiscoveryItem> {
    let external_id = text_at(aweme, &["aweme_id"]).or_else(|| text_at(aweme, &["id"]))?;
    let title = text_at(aweme, &["desc"])
        .or_else(|| text_at(aweme, &["caption"]))
        .or_else(|| text_at(aweme, &["title"]))
        .unwrap_or_default();
    let image_url = first_text_at(aweme, &["video", "cover", "url_list"])
        .or_else(|| first_text_at(aweme, &["video", "origin_cover", "url_list"]))
        .or_else(|| first_text_at(aweme, &["video", "dynamic_cover", "url_list"]))
        .or_else(|| text_at(aweme, &["thumbnail_url"]));
    let source_url = text_at(aweme, &["share_url"]).or_else(|| text_at(aweme, &["url"]));
    let author_handle = text_at(aweme, &["author", "unique_id"])
        .or_else(|| text_at(aweme, &["author", "nickname"]))
        .unwrap_or_default();
    let like_count = number_at(aweme, &["statistics", "digg_count"])
        .or_else(|| number_at(aweme, &["stats", "digg_count"]))
        .or_else(|| number_at(aweme, &["like_count"]));
    let source_published_at = text_at(aweme, &["create_time_utc"])
        .or_else(|| text_at(aweme, &["created_at"]))
        .or_else(|| number_at(aweme, &["create_time"]).and_then(unix_seconds_to_iso));

    Some(NormalizedDiscoveryItem {
        external_id,
        platform: "tiktok".to_string(),
        title,
        image_url,
        source_url,
        author_handle,
        like_count,
        source_published_at,
    })
}

fn normalize_instagram_reel(reel: &Value) -> Option<NormalizedDiscoveryItem> {
    let external_id = text_at(reel, &["shortcode"]).or_else(|| text_at(reel, &["id"]))?;
    let title = text_at(reel, &["caption", "text"])
        .or_else(|| text_at(reel, &["caption"]))
        .or_else(|| text_at(reel, &["title"]))
        .unwrap_or_default();
    let image_url = text_at(reel, &["thumbnail_url"])
        .or_else(|| text_at(reel, &["display_url"]))
        .or_else(|| text_at(reel, &["image_url"]));
    let source_url = text_at(reel, &["url"])
        .or_else(|| text_at(reel, &["permalink"]))
        .or_else(|| Some(format!("https://www.instagram.com/reel/{external_id}/")));
    let author_handle = text_at(reel, &["owner", "username"])
        .or_else(|| text_at(reel, &["user", "username"]))
        .or_else(|| text_at(reel, &["username"]))
        .unwrap_or_default();
    let like_count = number_at(reel, &["like_count"]).or_else(|| number_at(reel, &["likes"]));
    let source_published_at = text_at(reel, &["taken_at"])
        .or_else(|| number_at(reel, &["taken_at_timestamp"]).and_then(unix_seconds_to_iso));

    Some(NormalizedDiscoveryItem {
        external_id,
        platform: "instagram".to_string(),
        title,
        image_url,
        source_url,
        author_handle,
        like_count,
        source_published_at,
    })
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_array)
}

fn first_text_at(value: &Value, path: &[&str]) -> Option<String> {
    array_at(value, path)?
        .iter()
        .find_map(|item| item.as_str().map(ToString::to_string))
}

fn text_at(value: &Value, path: &[&str]) -> Option<String> {
    let value = path
        .iter()
        .try_fold(value, |current, key| current.get(*key))?;

    match value {
        Value::String(text) if !text.is_empty() => Some(text.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn number_at(value: &Value, path: &[&str]) -> Option<u64> {
    let value = path
        .iter()
        .try_fold(value, |current, key| current.get(*key))?;

    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

fn unix_seconds_to_iso(seconds: u64) -> Option<String> {
    let timestamp = OffsetDateTime::from_unix_timestamp(seconds.try_into().ok()?).ok()?;
    let format =
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].000Z").ok()?;
    timestamp.format(&format).ok()
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();

    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}
