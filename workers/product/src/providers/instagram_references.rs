use crate::domain::blitz::filter_synthetic_terms;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use time::{format_description, OffsetDateTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstagramFallbackPolicy {
    SkipVideos,
    AllowVideoThumbnails,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstagramImageCandidate {
    pub platform: String,
    pub source_handle: String,
    pub source_profile_id: Option<String>,
    pub source_post_id: String,
    pub source_post_code: String,
    pub source_image_index: u32,
    pub source_url: Option<String>,
    pub source_published_at: Option<String>,
    pub source_caption: Option<String>,
    pub media_type: u8,
    pub image_url: String,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub like_count: Option<u64>,
    pub comment_count: Option<u64>,
    pub play_count: Option<u64>,
    pub moodboard_id: String,
    pub moodboard_slug: String,
    pub discovered_via: String,
    pub raw_json: Value,
}

pub fn build_instagram_profile_url(base_url: &str, handle: &str) -> Result<String, &'static str> {
    let handle = clean_handle(handle).ok_or("missing_instagram_handle")?;
    Ok(format!(
        "{}/v1/instagram/profile?handle={}&trim=true",
        base_url.trim_end_matches('/'),
        url_encode(&handle)
    ))
}

pub fn build_instagram_user_posts_url(
    base_url: &str,
    handle: &str,
    next_max_id: Option<&str>,
) -> Result<String, &'static str> {
    let handle = clean_handle(handle).ok_or("missing_instagram_handle")?;
    let mut url = format!(
        "{}/v2/instagram/user/posts?handle={}",
        base_url.trim_end_matches('/'),
        url_encode(&handle)
    );
    if let Some(cursor) = next_max_id.map(str::trim).filter(|value| !value.is_empty()) {
        url.push_str("&next_max_id=");
        url.push_str(&url_encode(cursor));
    }
    url.push_str("&trim=true");
    Ok(url)
}

pub fn build_instagram_reels_search_url(
    base_url: &str,
    query: &str,
    page: Option<u32>,
) -> Result<String, &'static str> {
    let query = query.trim();
    if query.is_empty() {
        return Err("missing_instagram_reels_search_query");
    }
    let mut url = format!(
        "{}/v2/instagram/reels/search?query={}",
        base_url.trim_end_matches('/'),
        url_encode(query)
    );
    if let Some(page) = page.filter(|page| *page > 1) {
        url.push_str("&page=");
        url.push_str(&page.to_string());
    }
    url.push_str("&trim=true");
    Ok(url)
}

pub fn build_instagram_post_url(
    base_url: &str,
    post_url: &str,
    region: &str,
) -> Result<String, &'static str> {
    let post_url = post_url.trim();
    if !is_instagram_post_url(post_url) {
        return Err("invalid_instagram_post_url");
    }
    let requested_region = region.trim();
    if !requested_region.is_empty() && !requested_region.eq_ignore_ascii_case("US") {
        return Err("unsupported_instagram_region");
    }
    let region = "US";
    Ok(format!(
        "{}/v1/instagram/post?url={}&region={}&trim=true",
        base_url.trim_end_matches('/'),
        url_encode(post_url),
        url_encode(region)
    ))
}

pub fn normalize_instagram_profile_related_handles(raw: &Value, limit: usize) -> Vec<String> {
    if bool_from_value(raw.pointer("/data/user/is_private")).unwrap_or(false) {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    array_at(raw, &["data", "user", "edge_related_profiles", "edges"])
        .into_iter()
        .flatten()
        .filter_map(|edge| edge.get("node").unwrap_or(edge).as_object())
        .filter(|node| !bool_from_value(node.get("is_private")).unwrap_or(false))
        .filter_map(|node| node.get("username").and_then(Value::as_str))
        .filter_map(clean_handle)
        .filter(|handle| seen.insert(handle.to_ascii_lowercase()))
        .take(limit)
        .collect()
}

pub fn extract_instagram_reels_owner_handles(raw: &Value, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    instagram_reels_items(raw)
        .into_iter()
        .filter_map(instagram_reel_owner_handle)
        .filter_map(|handle| clean_handle(&handle))
        .filter(|handle| seen.insert(handle.to_ascii_lowercase()))
        .take(limit)
        .collect()
}

fn instagram_reels_items(raw: &Value) -> Vec<&Value> {
    array_at(raw, &["items"])
        .or_else(|| array_at(raw, &["reels"]))
        .or_else(|| array_at(raw, &["data"]))
        .into_iter()
        .flatten()
        .collect()
}

fn instagram_reel_owner_handle(reel: &Value) -> Option<String> {
    text_at(reel, &["user", "username"])
        .or_else(|| text_at(reel, &["owner", "username"]))
        .or_else(|| text_at(reel, &["username"]))
}

pub fn instagram_candidate_meets_min_dimensions(
    candidate: &InstagramImageCandidate,
    min_width: u32,
    min_height: u32,
) -> bool {
    candidate
        .image_width
        .map(|width| width >= min_width)
        .unwrap_or(true)
        && candidate
            .image_height
            .map(|height| height >= min_height)
            .unwrap_or(true)
}

pub fn normalize_instagram_user_posts(
    raw: &Value,
    fallback_handle: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    discovered_via: &str,
    fallback_policy: InstagramFallbackPolicy,
    images_per_post: usize,
) -> Vec<InstagramImageCandidate> {
    feed_items(raw)
        .into_iter()
        .flat_map(|item| {
            normalize_feed_item(
                item,
                fallback_handle,
                moodboard_id,
                moodboard_slug,
                discovered_via,
                fallback_policy,
                images_per_post,
            )
        })
        .collect()
}

pub fn normalize_instagram_post_detail(
    raw: &Value,
    fallback_handle: &str,
    source_url: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    discovered_via: &str,
    images_per_post: usize,
) -> Vec<InstagramImageCandidate> {
    normalize_instagram_post_detail_with_policy(
        raw,
        fallback_handle,
        source_url,
        moodboard_id,
        moodboard_slug,
        discovered_via,
        InstagramFallbackPolicy::SkipVideos,
        images_per_post,
    )
}

pub fn normalize_instagram_post_detail_with_policy(
    raw: &Value,
    fallback_handle: &str,
    source_url: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    discovered_via: &str,
    fallback_policy: InstagramFallbackPolicy,
    images_per_post: usize,
) -> Vec<InstagramImageCandidate> {
    let media = raw
        .pointer("/data/xdt_shortcode_media")
        .or_else(|| raw.pointer("/xdt_shortcode_media"))
        .unwrap_or(raw);
    let raw_id = text_at(media, &["id"]);
    let raw_shortcode = text_at(media, &["shortcode"]);
    let valid_raw_shortcode = raw_shortcode.clone().filter(|code| valid_shortcode(code));
    let source_url_shortcode = instagram_post_shortcode(source_url);
    let source_url_shortcode_for_identity = source_url_shortcode.clone();
    let Some(post_code) = valid_raw_shortcode
        .clone()
        .or(source_url_shortcode_for_identity)
        .or_else(|| raw_id.clone())
    else {
        return Vec::new();
    };
    let post_id = raw_id.unwrap_or_else(|| post_code.clone());
    let (source_handle, source_profile_id) = source_identity(media, fallback_handle);
    let caption = instagram_caption(media);
    if has_synthetic_caption(media) {
        return Vec::new();
    }

    let (image_sources, media_type) = image_sources_for_post_detail(media, fallback_policy);
    let source_url = normalized_instagram_source_url(source_url)
        .or_else(|| valid_raw_shortcode.map(|code| instagram_post_url_for_code(&code)))
        .or_else(|| source_url_shortcode.map(|code| instagram_post_url_for_code(&code)));

    image_sources
        .into_iter()
        .filter_map(|(source_index, source)| {
            best_image_for_value(source).map(|image| (source_index, source, image))
        })
        .take(images_per_post)
        .map(|(source_index, source, image)| InstagramImageCandidate {
            platform: "instagram".to_string(),
            source_handle: source_handle.clone(),
            source_profile_id: source_profile_id.clone(),
            source_post_id: post_id.clone(),
            source_post_code: post_code.clone(),
            source_image_index: source_index as u32,
            source_url: source_url.clone(),
            source_published_at: timestamp_at(media, &["taken_at_timestamp"])
                .or_else(|| timestamp_at(media, &["taken_at"])),
            source_caption: caption.clone(),
            media_type,
            image_url: image.url,
            image_width: image.width,
            image_height: image.height,
            like_count: number_at(media, &["edge_media_preview_like", "count"])
                .or_else(|| number_at(media, &["edge_liked_by", "count"]))
                .or_else(|| number_at(media, &["like_count"])),
            comment_count: number_at(media, &["edge_media_to_comment", "count"])
                .or_else(|| number_at(media, &["edge_media_preview_comment", "count"]))
                .or_else(|| number_at(media, &["edge_media_to_parent_comment", "count"]))
                .or_else(|| number_at(media, &["comment_count"])),
            play_count: number_at(media, &["video_play_count"])
                .or_else(|| number_at(media, &["video_view_count"]))
                .or_else(|| number_at(media, &["play_count"])),
            moodboard_id: moodboard_id.to_string(),
            moodboard_slug: moodboard_slug.to_string(),
            discovered_via: discovered_via.to_string(),
            raw_json: source.clone(),
        })
        .collect()
}

#[derive(Clone, Debug)]
struct ImageChoice {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

fn normalize_feed_item(
    item: &Value,
    fallback_handle: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    discovered_via: &str,
    fallback_policy: InstagramFallbackPolicy,
    images_per_post: usize,
) -> Vec<InstagramImageCandidate> {
    let has_sidecar = has_sidecar_metadata(item);
    let media_type = if has_sidecar {
        8
    } else {
        media_type_at(item, &["media_type"]).unwrap_or(0)
    };
    let is_video = media_type == 2 || is_video_media(item);
    if !has_sidecar && is_video && fallback_policy == InstagramFallbackPolicy::SkipVideos {
        return Vec::new();
    }
    let caption = feed_caption(item);
    if has_synthetic_caption(item) {
        return Vec::new();
    }

    let url_code = text_at(item, &["code"]).or_else(|| text_at(item, &["shortcode"]));
    let Some(post_code) = url_code.clone().or_else(|| text_at(item, &["id"])) else {
        return Vec::new();
    };
    let post_id = text_at(item, &["id"]).unwrap_or_else(|| post_code.clone());
    let (source_handle, source_profile_id) = feed_source_identity(item, fallback_handle);
    let source_url = text_at(item, &["url"])
        .and_then(|url| normalized_instagram_source_url(&url))
        .or_else(|| {
            url_code
                .filter(|code| valid_shortcode(code))
                .map(|code| instagram_post_url_for_code(&code))
        });
    let images = feed_item_images(item, media_type, is_video, fallback_policy, images_per_post);
    let media_type = feed_candidate_media_type(media_type, has_sidecar, is_video, fallback_policy);

    images
        .into_iter()
        .map(|(source_index, image)| InstagramImageCandidate {
            platform: "instagram".to_string(),
            source_handle: source_handle.clone(),
            source_profile_id: source_profile_id.clone(),
            source_post_id: post_id.clone(),
            source_post_code: post_code.clone(),
            source_image_index: source_index as u32,
            source_url: source_url.clone(),
            source_published_at: timestamp_at(item, &["taken_at"]),
            source_caption: caption.clone(),
            media_type,
            image_url: image.url,
            image_width: image.width,
            image_height: image.height,
            like_count: number_at(item, &["like_count"]),
            comment_count: number_at(item, &["comment_count"]),
            play_count: number_at(item, &["play_count"]),
            moodboard_id: moodboard_id.to_string(),
            moodboard_slug: moodboard_slug.to_string(),
            discovered_via: discovered_via.to_string(),
            raw_json: item.clone(),
        })
        .collect()
}

fn feed_candidate_media_type(
    media_type: u8,
    has_sidecar: bool,
    is_video: bool,
    fallback_policy: InstagramFallbackPolicy,
) -> u8 {
    if has_sidecar {
        8
    } else if is_video && fallback_policy == InstagramFallbackPolicy::AllowVideoThumbnails {
        2
    } else if media_type == 0 {
        1
    } else {
        media_type
    }
}

fn feed_item_images(
    item: &Value,
    media_type: u8,
    is_video: bool,
    fallback_policy: InstagramFallbackPolicy,
    images_per_post: usize,
) -> Vec<(usize, ImageChoice)> {
    if images_per_post == 0 {
        return Vec::new();
    }
    if media_type == 8 || has_sidecar_metadata(item) {
        return sidecar_children_with_positions(
            item,
            fallback_policy == InstagramFallbackPolicy::AllowVideoThumbnails,
        )
        .into_iter()
        .filter_map(|(index, child)| best_image_for_value(child).map(|image| (index, image)))
        .take(images_per_post)
        .collect();
    }
    if is_video && fallback_policy == InstagramFallbackPolicy::AllowVideoThumbnails {
        let explicit_thumbnail = text_at(item, &["thumbnail_url"])
            .or_else(|| text_at(item, &["display_uri"]))
            .or_else(|| text_at(item, &["display_url"]))
            .or_else(|| text_at(item, &["thumbnail_src"]))
            .filter(|url| image_url_is_allowed(url))
            .map(|url| ImageChoice {
                url,
                width: None,
                height: None,
            });
        return best_image_for_value(item)
            .or(explicit_thumbnail)
            .map(|image| (0, image))
            .into_iter()
            .collect();
    }
    best_image_for_value(item)
        .map(|image| (0, image))
        .into_iter()
        .collect()
}

fn feed_items(raw: &Value) -> Vec<&Value> {
    if let Some(items) = array_at(raw, &["items"]) {
        return items.iter().collect();
    }
    if let Some(items) = array_at(raw, &["data", "items"]) {
        return items.iter().collect();
    }
    if let Some(items) = array_at(raw, &["data"]) {
        return items.iter().collect();
    }
    Vec::new()
}

fn feed_caption(item: &Value) -> Option<String> {
    instagram_caption(item)
}

fn instagram_caption(item: &Value) -> Option<String> {
    instagram_captions(item).into_iter().next()
}

fn has_synthetic_caption(item: &Value) -> bool {
    instagram_captions(item)
        .iter()
        .any(|caption| filter_synthetic_terms(caption).is_err())
}

fn instagram_captions(item: &Value) -> Vec<String> {
    let mut captions = Vec::new();
    captions.extend(text_at(item, &["caption", "text"]));
    if let Some(edges) = array_at(item, &["edge_media_to_caption", "edges"]) {
        captions.extend(
            edges
                .iter()
                .filter_map(|edge| edge.get("node").unwrap_or(edge).get("text"))
                .filter_map(|text| text_at(text, &[])),
        );
    }
    captions.extend(text_at(item, &["caption"]));
    captions.extend(text_at(item, &["caption_text"]));
    captions
}

fn image_sources_for_post_detail(
    media: &Value,
    fallback_policy: InstagramFallbackPolicy,
) -> (Vec<(usize, &Value)>, u8) {
    if has_sidecar_metadata(media) {
        (
            sidecar_children_with_positions(
                media,
                fallback_policy == InstagramFallbackPolicy::AllowVideoThumbnails,
            ),
            8,
        )
    } else if !is_video_media(media) {
        (vec![(0, media)], 1)
    } else if fallback_policy == InstagramFallbackPolicy::AllowVideoThumbnails {
        (vec![(0, media)], 2)
    } else {
        (Vec::new(), 1)
    }
}

fn best_image_for_value(value: &Value) -> Option<ImageChoice> {
    let mut candidates = Vec::new();
    candidates.extend(
        array_at(value, &["image_versions2", "candidates"])
            .into_iter()
            .flatten()
            .filter_map(image_choice_from_value),
    );
    if let Some(additional) = value.pointer("/image_versions2/additional_candidates") {
        collect_additional_image_choices(additional, &mut candidates);
    }
    candidates.sort_by_key(|candidate| {
        candidate.width.unwrap_or(0) as u64 * candidate.height.unwrap_or(0) as u64
    });
    candidates.pop().or_else(|| {
        [
            "display_uri",
            "display_url",
            "thumbnail_src",
            "thumbnail_url",
            "image_url",
        ]
        .into_iter()
        .filter_map(|key| text_at(value, &[key]))
        .find(|url| image_url_is_allowed(url))
        .map(|url| ImageChoice {
            width: number_at(value, &["dimensions", "width"]).and_then(to_u32),
            height: number_at(value, &["dimensions", "height"]).and_then(to_u32),
            url,
        })
    })
}

fn collect_additional_image_choices(value: &Value, choices: &mut Vec<ImageChoice>) {
    if let Some(choice) = image_choice_from_value(value) {
        choices.push(choice);
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_additional_image_choices(item, choices);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_additional_image_choices(item, choices);
            }
        }
        _ => {}
    }
}

fn image_choice_from_value(value: &Value) -> Option<ImageChoice> {
    match value {
        Value::String(url) if image_url_is_allowed(url) => Some(ImageChoice {
            url: url.trim().to_string(),
            width: None,
            height: None,
        }),
        Value::Object(_) => {
            let url = text_at(value, &["url"])?;
            if !image_url_is_allowed(&url) {
                return None;
            }
            Some(ImageChoice {
                width: number_at(value, &["width"]).and_then(to_u32),
                height: number_at(value, &["height"]).and_then(to_u32),
                url,
            })
        }
        _ => None,
    }
}

fn sidecar_children_with_positions(
    value: &Value,
    include_video_children: bool,
) -> Vec<(usize, &Value)> {
    if let Some(edges) = array_at(value, &["edge_sidecar_to_children", "edges"]) {
        let children = collect_sidecar_children(
            edges.iter().map(|edge| edge.get("node").unwrap_or(edge)),
            include_video_children,
        );
        if !children.is_empty() {
            return children;
        }
    }
    if let Some(items) = array_at(value, &["carousel_media"]) {
        let children = collect_sidecar_children(items.iter(), include_video_children);
        if !children.is_empty() {
            return children;
        }
    }
    if let Some(items) = array_at(value, &["items"]) {
        let children = collect_sidecar_children(items.iter(), include_video_children);
        if !children.is_empty() {
            return children;
        }
    }
    Vec::new()
}

fn collect_sidecar_children<'a>(
    items: impl Iterator<Item = &'a Value>,
    include_video_children: bool,
) -> Vec<(usize, &'a Value)> {
    let mut children = Vec::new();
    let mut seen = HashSet::new();
    for (index, child) in items.enumerate() {
        if !include_video_children && is_video_media(child) {
            continue;
        }
        let Some(key) = sidecar_child_key(child) else {
            continue;
        };
        if seen.insert(key) {
            children.push((index, child));
        }
    }
    children
}

fn sidecar_child_key(child: &Value) -> Option<String> {
    let image = best_image_for_value(child)?;
    text_at(child, &["id"])
        .map(|id| format!("id:{id}"))
        .or_else(|| Some(format!("url:{}", image.url)))
}

fn has_sidecar_metadata(value: &Value) -> bool {
    array_at(value, &["edge_sidecar_to_children", "edges"])
        .map(|items| !items.is_empty())
        .unwrap_or(false)
        || array_at(value, &["carousel_media"])
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        || array_at(value, &["items"])
            .map(|items| items.iter().any(is_media_like_sidecar_child))
            .unwrap_or(false)
}

fn is_media_like_sidecar_child(value: &Value) -> bool {
    is_video_media(value)
        || best_image_for_value(value).is_some()
        || has_sidecar_metadata(value)
        || [
            "display_uri",
            "display_url",
            "thumbnail_src",
            "thumbnail_url",
            "image_url",
        ]
        .iter()
        .any(|key| value.get(*key).is_some())
        || value.pointer("/image_versions2/candidates").is_some()
        || value
            .pointer("/image_versions2/additional_candidates")
            .is_some()
}

fn url_is_profile_picture(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("profile_pic") || lower.contains("profilepic")
}

fn image_url_is_allowed(url: &str) -> bool {
    let url = url.trim();
    url.starts_with("https://") && !url_is_profile_picture(url)
}

fn clean_handle(handle: &str) -> Option<String> {
    let handle = handle.trim().trim_start_matches('@');
    (!handle.is_empty()
        && handle.len() <= 30
        && handle != "_"
        && !handle.starts_with('.')
        && !handle.ends_with('.')
        && !handle.contains("..")
        && handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_'))
    .then(|| handle.to_string())
}

fn feed_source_identity(value: &Value, fallback_handle: &str) -> (String, Option<String>) {
    source_identity(value, fallback_handle)
}

fn source_identity(value: &Value, fallback_handle: &str) -> (String, Option<String>) {
    identity_from_node(value.get("owner"))
        .or_else(|| identity_from_node(value.get("user")))
        .unwrap_or_else(|| (clean_handle(fallback_handle).unwrap_or_default(), None))
}

fn identity_from_node(value: Option<&Value>) -> Option<(String, Option<String>)> {
    let value = value?;
    let handle = text_at(value, &["username"]).and_then(|handle| clean_handle(&handle))?;
    let profile_id = text_at(value, &["id"]).or_else(|| text_at(value, &["pk"]));
    Some((handle, profile_id))
}

fn is_video_media(value: &Value) -> bool {
    let media_type = text_at(value, &["media_type"]);
    media_type
        .as_deref()
        .map(|value| value == "2" || value.eq_ignore_ascii_case("video"))
        .unwrap_or(false)
        || text_at(value, &["__typename"])
            .or_else(|| text_at(value, &["typename"]))
            .map(|typename| typename.to_ascii_lowercase().contains("video"))
            .unwrap_or(false)
        || value
            .get("is_video")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || value
            .get("video_versions")
            .and_then(Value::as_array)
            .map(|versions| !versions.is_empty())
            .unwrap_or(false)
        || has_meaningful_value(value.get("video_url"))
        || has_meaningful_value(value.get("video"))
        || has_meaningful_value(value.get("video_dash_manifest"))
}

fn has_meaningful_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

fn is_instagram_post_url(url: &str) -> bool {
    instagram_post_shortcode(url).is_some()
}

fn normalized_instagram_source_url(url: &str) -> Option<String> {
    let url = url.trim();
    is_instagram_post_url(url).then(|| url.to_string())
}

fn instagram_post_url_for_code(code: &str) -> String {
    format!("https://www.instagram.com/p/{code}/")
}

fn instagram_post_shortcode(url: &str) -> Option<String> {
    let Some(rest) = url.strip_prefix("https://") else {
        return None;
    };
    let without_fragment = rest.split('#').next().unwrap_or(rest);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((host, path)) = without_query.split_once('/') else {
        return None;
    };
    let host = host.to_ascii_lowercase();
    if host != "instagram.com" && host != "www.instagram.com" {
        return None;
    }
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    if !matches!(segments.next(), Some("p" | "reel" | "tv")) {
        return None;
    }
    let shortcode = segments.next()?;
    (valid_shortcode(shortcode) && segments.next().is_none()).then(|| shortcode.to_string())
}

fn valid_shortcode(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let value = path.iter().try_fold(value, |current, key| {
        if let Ok(index) = key.parse::<usize>() {
            current.get(index)
        } else {
            current.get(*key)
        }
    })?;
    value.as_array()
}

fn text_at(value: &Value, path: &[&str]) -> Option<String> {
    let value = path.iter().try_fold(value, |current, key| {
        if let Ok(index) = key.parse::<usize>() {
            current.get(index)
        } else {
            current.get(*key)
        }
    })?;
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn number_at(value: &Value, path: &[&str]) -> Option<u64> {
    let value = path.iter().try_fold(value, |current, key| {
        if let Ok(index) = key.parse::<usize>() {
            current.get(index)
        } else {
            current.get(*key)
        }
    })?;
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

fn to_u32(value: u64) -> Option<u32> {
    u32::try_from(value).ok()
}

fn bool_from_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Some(Value::Number(number)) => number.as_u64().and_then(|value| match value {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }),
        _ => None,
    }
}

fn media_type_at(value: &Value, path: &[&str]) -> Option<u8> {
    number_at(value, path).and_then(|value| u8::try_from(value).ok())
}

fn timestamp_at(value: &Value, path: &[&str]) -> Option<String> {
    match path
        .iter()
        .try_fold(value, |current, key| current.get(*key))?
    {
        Value::Number(number) => number.as_i64().and_then(unix_seconds_to_iso),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            text.parse::<i64>()
                .ok()
                .and_then(unix_seconds_to_iso)
                .or_else(|| Some(text.to_string()))
        }
        _ => None,
    }
}

fn unix_seconds_to_iso(seconds: i64) -> Option<String> {
    let timestamp = OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    let format =
        format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].000Z").ok()?;
    timestamp.format(&format).ok()
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
