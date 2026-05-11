use mirai_product_worker::ai::model_router::{choose_model, clamp_moderation_level, ModelConfig};
use mirai_product_worker::ai::tasks::AiTask;
use mirai_product_worker::domain::entitlements::{can_create_clone, Entitlements};
use mirai_product_worker::domain::idempotency::clone_upload_key;
use mirai_product_worker::domain::media_validation::{
    is_supported_reference_content_type, validate_reference_count, ReferenceCountError,
};
use mirai_product_worker::domain::status::{can_transition_soul_status, SoulStatus};
use mirai_product_worker::routes::onboarding::default_bubbles;
use mirai_product_worker::services::accounts::{
    account_checkout_enabled, account_entitlement_snapshot, account_portal_enabled,
    account_usage_limits, VerifiedIdentity,
};
use mirai_product_worker::services::clones::{handle_with_suffix, slugify_handle};
use mirai_product_worker::services::media::{media_storage_key, normalize_extension, safe_segment};
use mirai_product_worker::services::provider_accounts::{
    choose_provider_account, ProviderAccountCandidate,
};
use mirai_product_worker::providers::scrapecreators::{
    build_scrape_request, normalize_instagram_reels_search, normalize_tiktok_keyword_search,
    ScrapePlatform,
};
use serde_json::json;

#[test]
fn text_only_models_are_not_chosen_for_vision_tasks() {
    let text_only = vec![ModelConfig {
        provider: "deepseek".to_string(),
        model: "deepseek-v4-pro".to_string(),
        supports_vision: false,
        supports_structured_json: true,
    }];
    let models = vec![
        ModelConfig {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            supports_vision: false,
            supports_structured_json: true,
        },
        ModelConfig {
            provider: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];

    assert!(choose_model(AiTask::PhotoQualityReview, &text_only).is_none());

    let selected = choose_model(AiTask::PhotoQualityReview, &models).unwrap();

    assert_eq!(selected.model, "gpt-4.1-mini");
    assert!(selected.supports_vision);
}

#[test]
fn visual_reference_selection_uses_vision_models() {
    let models = vec![
        ModelConfig {
            provider: "openrouter".to_string(),
            model: "deepseek/deepseek-v4-pro".to_string(),
            supports_vision: false,
            supports_structured_json: true,
        },
        ModelConfig {
            provider: "workers_ai".to_string(),
            model: "@cf/moonshotai/kimi-k2.6".to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];

    let selected = choose_model(AiTask::VisualReferenceSelection, &models).unwrap();

    assert_eq!(selected.provider, "workers_ai");
}

#[test]
fn models_without_structured_json_are_rejected() {
    let models = vec![ModelConfig {
        provider: "workers_ai".to_string(),
        model: "@cf/moonshotai/kimi-k2.6".to_string(),
        supports_vision: true,
        supports_structured_json: false,
    }];

    assert!(choose_model(AiTask::HumanPresenceDetection, &models).is_none());
}

#[test]
fn deepseek_can_handle_text_tasks() {
    let models = vec![ModelConfig {
        provider: "deepseek".to_string(),
        model: "deepseek-v4-pro".to_string(),
        supports_vision: false,
        supports_structured_json: true,
    }];

    let selected = choose_model(AiTask::NicheSeedExtraction, &models).unwrap();

    assert_eq!(selected.provider, "deepseek");
    assert_eq!(selected.model, "deepseek-v4-pro");
}

#[test]
fn moderation_level_is_bounded() {
    assert_eq!(clamp_moderation_level(-3), 0);
    assert_eq!(clamp_moderation_level(7), 7);
    assert_eq!(clamp_moderation_level(42), 10);
}

#[test]
fn default_bubbles_include_visual_queries() {
    let bubbles = default_bubbles();

    assert!(bubbles.len() >= 8);
    assert!(bubbles
        .iter()
        .all(|bubble| !bubble.search_queries.is_empty()));
    assert!(bubbles.iter().any(|bubble| bubble.slug == "y2k-cafe"));
}

#[test]
fn free_users_can_create_only_one_active_clone() {
    let free = Entitlements {
        max_active_clones: 1,
    };
    assert!(can_create_clone(&free, 0).is_ok());
    assert_eq!(
        can_create_clone(&free, 1).unwrap_err(),
        "clone_limit_reached"
    );
}

#[test]
fn paid_users_can_create_up_to_five_active_clones() {
    let paid = Entitlements {
        max_active_clones: 5,
    };
    assert!(can_create_clone(&paid, 4).is_ok());
    assert_eq!(
        can_create_clone(&paid, 5).unwrap_err(),
        "clone_limit_reached"
    );
}

#[test]
fn production_entitlement_policy_maps_free_and_paid_limits() {
    let free = Entitlements::free();
    let paid = Entitlements::paid();

    assert_eq!(free.max_active_clones, 1);
    assert_eq!(paid.max_active_clones, 5);
    assert!(can_create_clone(&free, 0).is_ok());
    assert_eq!(
        can_create_clone(&free, 1).unwrap_err(),
        "clone_limit_reached"
    );
    assert!(can_create_clone(&paid, 4).is_ok());
    assert_eq!(
        can_create_clone(&paid, 5).unwrap_err(),
        "clone_limit_reached"
    );
}

#[test]
fn reference_count_must_match_higgsfield_range() {
    assert_eq!(
        validate_reference_count(4),
        Err(ReferenceCountError::TooFew)
    );
    assert_eq!(validate_reference_count(5), Ok(()));
    assert_eq!(validate_reference_count(20), Ok(()));
    assert_eq!(
        validate_reference_count(21),
        Err(ReferenceCountError::TooMany)
    );
}

#[test]
fn supported_reference_content_types_match_upload_policy() {
    for content_type in [
        "image/jpeg",
        "image/jpg",
        "image/png",
        "image/webp",
        "image/heic",
        "image/heif",
        "IMAGE/JPEG",
        "Image/Png",
        "image/jpeg; charset=binary",
    ] {
        assert!(
            is_supported_reference_content_type(content_type),
            "{content_type} should be supported"
        );
    }

    for content_type in ["text/plain", "image/gif", "application/octet-stream"] {
        assert!(
            !is_supported_reference_content_type(content_type),
            "{content_type} should be rejected"
        );
    }
}

#[test]
fn clone_upload_idempotency_key_is_stable() {
    let a = clone_upload_key(
        "user_1",
        "My Soul",
        &["hash_b".to_string(), "hash_a".to_string()],
    );
    let b = clone_upload_key(
        "user_1",
        "My Soul",
        &["hash_a".to_string(), "hash_b".to_string()],
    );
    assert_eq!(a, b);
    assert!(a.starts_with("clone_upload:user_1:"));
}

#[test]
fn soul_status_transitions_are_explicit() {
    assert!(can_transition_soul_status(
        SoulStatus::Queued,
        SoulStatus::Training
    ));
    assert!(can_transition_soul_status(
        SoulStatus::Training,
        SoulStatus::Ready
    ));
    assert!(can_transition_soul_status(
        SoulStatus::Training,
        SoulStatus::Failed
    ));
    assert!(!can_transition_soul_status(
        SoulStatus::Ready,
        SoulStatus::Training
    ));
}

#[test]
fn account_usage_limits_come_from_verified_identity() {
    let identity = VerifiedIdentity {
        user_id: "user_1".to_string(),
        email: Some("creator@example.com".to_string()),
        name: Some("Creator".to_string()),
        plan: "paid".to_string(),
        max_active_clones: 5,
        generation_priority: "high".to_string(),
        watermark_exports: false,
    };

    let limits = account_usage_limits(&identity, 3);
    assert_eq!(limits.active_clones, 3);
    assert_eq!(limits.max_active_clones, 5);
    assert_eq!(limits.plan, "paid");
}

#[test]
fn account_entitlement_snapshot_preserves_verified_identity_fields() {
    let identity = VerifiedIdentity {
        user_id: "user_1".to_string(),
        email: Some("creator@example.com".to_string()),
        name: Some("Creator".to_string()),
        plan: "free".to_string(),
        max_active_clones: 7,
        generation_priority: "verified-priority".to_string(),
        watermark_exports: false,
    };

    let snapshot = account_entitlement_snapshot(&identity);
    assert_eq!(snapshot.max_active_clones, 7);
    assert_eq!(snapshot.generation_priority, "verified-priority");
    assert!(!snapshot.watermark_exports);
}

#[test]
fn account_snapshots_serialize_public_json_as_camel_case() {
    let identity = VerifiedIdentity {
        user_id: "user_1".to_string(),
        email: Some("creator@example.com".to_string()),
        name: Some("Creator".to_string()),
        plan: "free".to_string(),
        max_active_clones: 7,
        generation_priority: "verified-priority".to_string(),
        watermark_exports: false,
    };

    assert_eq!(
        serde_json::to_value(account_entitlement_snapshot(&identity)).unwrap(),
        json!({
            "maxActiveClones": 7,
            "generationPriority": "verified-priority",
            "watermarkExports": false,
        })
    );
    assert_eq!(
        serde_json::to_value(account_usage_limits(&identity, 3)).unwrap(),
        json!({
            "activeClones": 3,
            "maxActiveClones": 7,
            "plan": "free",
        })
    );
}

#[test]
fn account_billing_flags_default_false_and_follow_config() {
    assert!(!account_checkout_enabled(None, None, None, None));
    assert!(!account_portal_enabled(None, None));

    assert!(!account_checkout_enabled(
        None,
        None,
        Some("prod_pro"),
        None
    ));
    assert!(!account_checkout_enabled(
        None,
        Some("polar_token"),
        None,
        None
    ));
    assert!(account_checkout_enabled(
        None,
        Some("polar_token"),
        Some("prod_pro"),
        None
    ));
    assert!(account_checkout_enabled(
        None,
        Some("polar_token"),
        None,
        Some("prod_studio")
    ));
    assert!(account_checkout_enabled(Some("true"), None, None, None));
    assert!(!account_checkout_enabled(
        Some("false"),
        Some("polar_token"),
        Some("prod_pro"),
        None
    ));

    assert!(account_portal_enabled(None, Some("polar_token")));
    assert!(account_portal_enabled(Some("true"), None));
    assert!(!account_portal_enabled(Some("false"), Some("polar_token")));
}

#[test]
fn media_storage_key_is_user_scoped() {
    let key = media_storage_key("user/one", "clone:two", "media_abc", "image/png");
    assert_eq!(key, "users/user-one/clones/clone-two/media_abc.png");
}

#[test]
fn provider_selection_skips_unhealthy_accounts() {
    let candidates = vec![
        ProviderAccountCandidate {
            id: "bad".to_string(),
            health_state: "auth_required".to_string(),
            active_leases: 0,
            max_leases: 2,
        },
        ProviderAccountCandidate {
            id: "good".to_string(),
            health_state: "healthy".to_string(),
            active_leases: 1,
            max_leases: 2,
        },
    ];
    assert_eq!(choose_provider_account(&candidates).unwrap().id, "good");
}

#[test]
fn media_safe_segment_is_exported_deterministic_and_capped() {
    let input = "user/with:unsafe spaces".repeat(10);
    let segment = safe_segment(&input);
    assert_eq!(segment.len(), 96);
    assert!(segment.contains("user-with-unsafe-spaces"));
    assert!(segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')));
    assert_eq!(safe_segment(""), "segment");
    assert_eq!(safe_segment("."), "segment");
    assert_eq!(safe_segment(".."), "segment");
}

#[test]
fn normalize_extension_uses_content_type() {
    assert_eq!(normalize_extension("image/jpeg"), "jpg");
    assert_eq!(normalize_extension("image/png"), "png");
    assert_eq!(normalize_extension("image/webp"), "webp");
    assert_eq!(normalize_extension("image/heic"), "heic");
}

#[test]
fn clone_handle_slug_is_stable() {
    assert_eq!(slugify_handle("My New Soul!!"), "my-new-soul");
    assert_eq!(slugify_handle("!!My Soul"), "my-soul");
    assert_eq!(slugify_handle("   "), "my-soul");
}

#[test]
fn clone_handle_suffix_preserves_length_limit() {
    let base = "a".repeat(48);

    assert_eq!(handle_with_suffix(&base, 1), base);
    assert_eq!(
        handle_with_suffix(&base, 12),
        format!("{}-12", "a".repeat(45))
    );
    assert_eq!(handle_with_suffix("my-soul", 3), "my-soul-3");
}

#[test]
fn scrape_request_builder_allows_only_tiktok_and_instagram() {
    let tiktok = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::TikTokKeyword,
        "streetwear fit",
        "US",
    )
    .unwrap();
    assert_eq!(
        tiktok,
        "https://api.scrapecreators.com/v1/tiktok/search/keyword?query=streetwear%20fit&sort_by=date-posted&date_posted=last-6-months&trim=true&region=US"
    );

    let instagram = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::InstagramReels,
        "clean girl morning",
        "US",
    )
    .unwrap();
    assert_eq!(
        instagram,
        "https://api.scrapecreators.com/v2/instagram/reels/search?query=clean%20girl%20morning&date_posted=last-year"
    );
}

#[test]
fn tiktok_keyword_normalizer_extracts_recent_image_candidates() {
    let raw = serde_json::json!({
        "search_item_list": [{
            "aweme_info": {
                "aweme_id": "725",
                "desc": "city mirror fit",
                "create_time": 1767225600,
                "create_time_utc": "2026-01-01T00:00:00.000Z",
                "share_url": "https://www.tiktok.com/@creator/video/725",
                "statistics": { "digg_count": 23456 },
                "author": { "unique_id": "creator" },
                "video": {
                    "cover": { "url_list": ["https://cdn.example/cover.jpg"] }
                }
            }
        }]
    });

    let items = normalize_tiktok_keyword_search(&raw);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].external_id, "725");
    assert_eq!(items[0].platform, "tiktok");
    assert_eq!(items[0].title, "city mirror fit");
    assert_eq!(items[0].like_count, Some(23456));
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    assert_eq!(
        items[0].image_url.as_deref(),
        Some("https://cdn.example/cover.jpg")
    );
}

#[test]
fn instagram_reels_normalizer_extracts_reel_candidates() {
    let raw = serde_json::json!({
        "reels": [{
            "shortcode": "ABC123",
            "caption": { "text": "neutral outfit morning" },
            "thumbnail_url": "https://cdn.example/ig.jpg",
            "url": "https://www.instagram.com/reel/ABC123/",
            "like_count": 6000,
            "owner": { "username": "igcreator" },
            "taken_at": "2026-02-03T04:05:06.000Z"
        }]
    });

    let items = normalize_instagram_reels_search(&raw);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].external_id, "ABC123");
    assert_eq!(items[0].platform, "instagram");
    assert_eq!(items[0].author_handle, "igcreator");
    assert_eq!(items[0].like_count, Some(6000));
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-02-03T04:05:06.000Z")
    );
}

