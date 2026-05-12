use mirai_product_worker::ai::model_router::{choose_model, clamp_moderation_level, ModelConfig};
use mirai_product_worker::ai::tasks::AiTask;
use mirai_product_worker::ai::workers_ai::{
    human_presence_prompt, knowledge_extraction_prompt, seed_extraction_prompt, KIMI_K2_6_MODEL,
};
use mirai_product_worker::domain::blitz::{
    accumulate_influence, can_accept_human_presence, classify_freshness, daily_generation_limit,
    filter_synthetic_terms, select_visual_references, FreshnessDecision, HumanPresenceReview,
    Influence, SwipeMetadata, VisualReferenceForSelection,
};
use mirai_product_worker::domain::entitlements::{can_create_clone, Entitlements};
use mirai_product_worker::domain::idempotency::clone_upload_key;
use mirai_product_worker::domain::media_validation::{
    is_supported_reference_content_type, validate_reference_count, ReferenceCountError,
};
use mirai_product_worker::domain::status::{can_transition_soul_status, SoulStatus};
use mirai_product_worker::routes::onboarding::default_bubbles;
use mirai_product_worker::scrapecreators::{
    build_scrape_request, normalize_instagram_reels_search, normalize_tiktok_keyword_search,
    scrape_platform_from_str, ScrapePlatform,
};
use mirai_product_worker::services::accounts::{
    account_checkout_enabled, account_entitlement_snapshot, account_portal_enabled,
    account_usage_limits, VerifiedIdentity,
};
use mirai_product_worker::services::blitz::{
    next_batch_should_trigger, stored_batch_size_for_selected_refs, swipe_action_to_db_value,
    trigger_influence_cutoff_batch_number,
};
use mirai_product_worker::services::clones::{handle_with_suffix, slugify_handle};
use mirai_product_worker::services::media::{media_storage_key, normalize_extension, safe_segment};
use mirai_product_worker::services::provider_accounts::{
    choose_provider_account, ProviderAccountCandidate,
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
            provider: "workers_ai".to_string(),
            model: "@cf/moonshotai/kimi-k2.6".to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];

    assert!(choose_model(AiTask::PhotoQualityReview, &text_only).is_none());

    let selected = choose_model(AiTask::PhotoQualityReview, &models).unwrap();

    assert_eq!(selected.provider, "workers_ai");
    assert_eq!(selected.model, "@cf/moonshotai/kimi-k2.6");
    assert!(selected.supports_vision);
}

#[test]
fn blitz_swipe_actions_accept_like_and_dislike_only() {
    assert_eq!(swipe_action_to_db_value("like").unwrap(), "like");
    assert_eq!(swipe_action_to_db_value("dislike").unwrap(), "dislike");
    assert_eq!(
        swipe_action_to_db_value("pass").unwrap_err(),
        "invalid_swipe_action"
    );
}

#[test]
fn first_swipe_triggers_prefetch_once() {
    assert!(next_batch_should_trigger(0));
    assert!(!next_batch_should_trigger(1));
    assert!(!next_batch_should_trigger(4));
}

#[test]
fn influence_for_next_batch_skips_current_batch_feedback() {
    assert_eq!(trigger_influence_cutoff_batch_number(1), 0);
    assert_eq!(trigger_influence_cutoff_batch_number(2), 0);
    assert_eq!(trigger_influence_cutoff_batch_number(3), 1);
    assert_eq!(trigger_influence_cutoff_batch_number(4), 2);
}

#[test]
fn partial_blitz_batches_store_selected_reference_count() {
    assert_eq!(stored_batch_size_for_selected_refs(5, 3), 3);
    assert_eq!(stored_batch_size_for_selected_refs(5, 5), 5);
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
fn kimi_is_the_only_analysis_model_for_app_analysis_tasks() {
    let models = vec![
        ModelConfig {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            supports_vision: false,
            supports_structured_json: true,
        },
        ModelConfig {
            provider: "workers_ai".to_string(),
            model: KIMI_K2_6_MODEL.to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];

    for task in [
        AiTask::PhotoQualityReview,
        AiTask::HumanPresenceDetection,
        AiTask::BubbleGeneration,
        AiTask::NicheSeedExtraction,
        AiTask::NicheKnowledgeExtraction,
        AiTask::NicheClusterExpansion,
        AiTask::VisualReferenceSelection,
        AiTask::Moderation,
    ] {
        let selected = choose_model(task, &models).unwrap();

        assert_eq!(selected.provider, "workers_ai");
        assert_eq!(selected.model, KIMI_K2_6_MODEL);
    }
}

#[test]
fn workers_ai_prompts_include_research_guardrails() {
    let seed = seed_extraction_prompt("Clean Girl Street", &["minimal outfit".to_string()]);
    assert!(seed.contains("TikTok and Instagram"));
    assert!(seed.contains("Do not include synthetic/generation topics"));

    let knowledge = knowledge_extraction_prompt("Clean Girl Street");
    assert!(knowledge.contains("Do not extract from known-stale source items"));

    let human = human_presence_prompt();
    assert!(human.contains("exactly one human person"));
    assert!(human.contains("organic creator content"));
    assert!(human.contains("render_like"));
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
fn synthetic_generation_terms_are_rejected_case_insensitively() {
    assert!(filter_synthetic_terms("clean girl outfit inspo").is_ok());
    assert!(filter_synthetic_terms("paid creator outfit").is_ok());
    assert_eq!(
        filter_synthetic_terms("ai").unwrap_err(),
        "synthetic_generation_term"
    );
    assert_eq!(
        filter_synthetic_terms("AI generated avatar inspo").unwrap_err(),
        "synthetic_generation_term"
    );
    assert_eq!(
        filter_synthetic_terms("Midjourney fashion render").unwrap_err(),
        "synthetic_generation_term"
    );
}

#[test]
fn source_freshness_uses_rolling_five_year_cutoff() {
    assert_eq!(
        classify_freshness(
            Some("2024-02-01T00:00:00.000Z"),
            true,
            "2026-05-11T00:00:00.000Z",
            5
        ),
        FreshnessDecision::Recent
    );
    assert_eq!(
        classify_freshness(
            Some("2020-05-10T00:00:00.000Z"),
            true,
            "2026-05-11T00:00:00.000Z",
            5
        ),
        FreshnessDecision::TooOld
    );
    assert_eq!(
        classify_freshness(
            Some("2026-05-12T00:00:00.000Z"),
            true,
            "2026-05-11T00:00:00.000Z",
            5
        ),
        FreshnessDecision::TooOld
    );
    assert_eq!(
        classify_freshness(None, true, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::UnknownAllowed
    );
    assert_eq!(
        classify_freshness(None, false, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::UnknownRejected
    );
}

#[test]
fn human_presence_accepts_single_organic_recent_images_only() {
    let accepted = HumanPresenceReview {
        has_human: true,
        human_count: 1,
        human_type: "full_body".to_string(),
        confidence: 0.82,
        organic_photo_score: 0.8,
        freshness_visual_score: 0.78,
        capture_style: "phone".to_string(),
        aesthetic_tags: vec!["street".to_string()],
        rejection_reason: None,
    };
    assert!(can_accept_human_presence(&accepted).is_ok());

    let mut multiple = accepted.clone();
    multiple.human_count = 2;
    assert_eq!(
        can_accept_human_presence(&multiple).unwrap_err(),
        "multiple_humans"
    );

    let mut studio = accepted.clone();
    studio.capture_style = "professional_studio".to_string();
    assert_eq!(
        can_accept_human_presence(&studio).unwrap_err(),
        "too_professional"
    );

    let mut no_human = accepted.clone();
    no_human.has_human = false;
    no_human.human_count = 0;
    assert_eq!(
        can_accept_human_presence(&no_human).unwrap_err(),
        "no_human"
    );

    let mut low_confidence = accepted.clone();
    low_confidence.confidence = 0.69;
    assert_eq!(
        can_accept_human_presence(&low_confidence).unwrap_err(),
        "low_confidence"
    );

    let mut stale_visual_style = accepted.clone();
    stale_visual_style.freshness_visual_score = 0.69;
    assert_eq!(
        can_accept_human_presence(&stale_visual_style).unwrap_err(),
        "not_recent_visual_style"
    );

    let mut render_like = accepted.clone();
    render_like.capture_style = "render_like".to_string();
    assert_eq!(
        can_accept_human_presence(&render_like).unwrap_err(),
        "too_professional"
    );

    let mut mannequin = accepted.clone();
    mannequin.human_type = "mannequin".to_string();
    assert_eq!(
        can_accept_human_presence(&mannequin).unwrap_err(),
        "unsupported_human_type"
    );

    let mut hands_only = accepted.clone();
    hands_only.human_type = "hands_only".to_string();
    assert_eq!(
        can_accept_human_presence(&hands_only).unwrap_err(),
        "unsupported_human_type"
    );

    let mut invalid_confidence = accepted.clone();
    invalid_confidence.confidence = f64::NAN;
    assert_eq!(
        can_accept_human_presence(&invalid_confidence).unwrap_err(),
        "invalid_score"
    );

    let mut infinite_organic = accepted.clone();
    infinite_organic.organic_photo_score = f64::INFINITY;
    assert_eq!(
        can_accept_human_presence(&infinite_organic).unwrap_err(),
        "invalid_score"
    );

    let mut out_of_range_freshness = accepted.clone();
    out_of_range_freshness.freshness_visual_score = 1.01;
    assert_eq!(
        can_accept_human_presence(&out_of_range_freshness).unwrap_err(),
        "invalid_score"
    );
}

#[test]
fn daily_generation_limits_follow_plan() {
    assert_eq!(daily_generation_limit("free", 10, 50), 10);
    assert_eq!(daily_generation_limit("paid", 10, 50), 50);
    assert_eq!(daily_generation_limit("pro", 10, 50), 50);
    assert_eq!(daily_generation_limit("pro ", 10, 50), 50);
    assert_eq!(daily_generation_limit("studio", 10, 50), 50);
    assert_eq!(daily_generation_limit("unknown", 10, 50), 10);
}

#[test]
fn influence_accumulates_likes_and_dislikes_from_metadata() {
    let influence = accumulate_influence(&[
        SwipeMetadata {
            action: "like".to_string(),
            aesthetic_tags: vec!["minimalist".to_string(), "street".to_string()],
            niche_cluster: Some("outfit-inspo".to_string()),
            source_platform: "tiktok".to_string(),
            visual_reference_id: Some("vref_1".to_string()),
        },
        SwipeMetadata {
            action: "dislike".to_string(),
            aesthetic_tags: vec!["neon".to_string()],
            niche_cluster: Some("formal-wear".to_string()),
            source_platform: "instagram".to_string(),
            visual_reference_id: Some("vref_2".to_string()),
        },
    ]);

    assert_eq!(influence.liked_tags["minimalist"], 1);
    assert_eq!(influence.liked_clusters["outfit-inspo"], 1);
    assert_eq!(influence.disliked_tags["neon"], 1);
    assert_eq!(influence.disliked_clusters["formal-wear"], 1);
    assert_eq!(influence.liked_platforms["tiktok"], 1);
    assert_eq!(influence.liked_visual_reference_ids["vref_1"], 1);
}

#[test]
fn influence_normalizes_text_keys_but_preserves_reference_id_case() {
    let influence = accumulate_influence(&[SwipeMetadata {
        action: "like".to_string(),
        aesthetic_tags: vec![" Minimalist ".to_string()],
        niche_cluster: Some(" Outfit-Inspo ".to_string()),
        source_platform: "TikTok".to_string(),
        visual_reference_id: Some(" VRef_A ".to_string()),
    }]);

    assert_eq!(influence.liked_tags["minimalist"], 1);
    assert_eq!(influence.liked_clusters["outfit-inspo"], 1);
    assert_eq!(influence.liked_platforms["tiktok"], 1);
    assert_eq!(influence.liked_visual_reference_ids["VRef_A"], 1);

    let refs = vec![
        VisualReferenceForSelection {
            id: "aaa_unmatched".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("other".to_string()),
            aesthetic_tags: vec!["other".to_string()],
            human_presence_score: 0.8,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "zzz_matched".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("outfit-inspo".to_string()),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.8,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        },
    ];

    let selected = select_visual_references(&refs, &influence, 1, 4, "2026-05-11T00:00:00.000Z");

    assert_eq!(selected[0].id, "zzz_matched");
}

#[test]
fn selection_respects_influence_variety_and_reuse_cap() {
    let refs = vec![
        VisualReferenceForSelection {
            id: "liked_repeat".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2025-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("outfit-inspo".to_string()),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.8,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 1,
            last_liked_at: Some("2026-05-10T00:00:00.000Z".to_string()),
        },
        VisualReferenceForSelection {
            id: "unliked_used".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2025-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("outfit-inspo".to_string()),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.95,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 1,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "fresh_unused".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("mirror-fit".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.7,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "capped".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("mirror-fit".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.9,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 4,
            last_liked_at: Some("2026-05-10T00:00:00.000Z".to_string()),
        },
    ];
    let mut influence = Influence::default();
    influence.liked_tags.insert("minimalist".to_string(), 3);
    influence
        .liked_visual_reference_ids
        .insert("liked_repeat".to_string(), 1);

    let selected = select_visual_references(&refs, &influence, 2, 4, "2026-05-11T00:00:00.000Z");
    let ids = selected.into_iter().map(|item| item.id).collect::<Vec<_>>();

    assert_eq!(
        ids,
        vec!["liked_repeat".to_string(), "fresh_unused".to_string()]
    );
    assert!(!ids.contains(&"unliked_used".to_string()));
    assert!(!ids.contains(&"capped".to_string()));
}

#[test]
fn selection_filters_invalid_scores_and_future_sources() {
    let refs = vec![
        VisualReferenceForSelection {
            id: "valid".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("valid".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.7,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "nan_score".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("invalid".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: f64::NAN,
            organic_photo_score: 1.0,
            freshness_visual_score: 1.0,
            generation_use_count: 0,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "future".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-05-12T00:00:00.000Z".to_string()),
            niche_cluster: Some("future".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.95,
            organic_photo_score: 0.95,
            freshness_visual_score: 0.95,
            generation_use_count: 0,
            last_liked_at: None,
        },
    ];

    let selected = select_visual_references(
        &refs,
        &Influence::default(),
        3,
        4,
        "2026-05-11T00:00:00.000Z",
    );
    let ids = selected.into_iter().map(|item| item.id).collect::<Vec<_>>();

    assert_eq!(ids, vec!["valid".to_string()]);
}

#[test]
fn selection_caps_cluster_and_platform_variety() {
    let refs = (0..6)
        .map(|index| VisualReferenceForSelection {
            id: format!("same_cluster_{index}"),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("same-cluster".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.9 - (index as f64 * 0.01),
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        })
        .chain((0..5).map(|index| VisualReferenceForSelection {
            id: format!("same_platform_{index}"),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some(format!("cluster_{index}")),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.85 - (index as f64 * 0.01),
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        }))
        .collect::<Vec<_>>();

    let selected = select_visual_references(
        &refs,
        &Influence::default(),
        8,
        4,
        "2026-05-11T00:00:00.000Z",
    );

    let same_cluster_count = selected
        .iter()
        .filter(|item| item.niche_cluster.as_deref() == Some("same-cluster"))
        .count();
    let instagram_count = selected
        .iter()
        .filter(|item| item.source_platform == "instagram")
        .count();

    assert_eq!(same_cluster_count, 2);
    assert_eq!(instagram_count, 3);
    assert!(selected.len() <= 5);
}

#[test]
fn freshness_decision_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_value(FreshnessDecision::UnknownAllowed).unwrap(),
        json!("unknown_allowed")
    );
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

    let hashtag = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::TikTokHashtag,
        "streetwear",
        "US",
    )
    .unwrap();
    assert_eq!(
        hashtag,
        "https://api.scrapecreators.com/v1/tiktok/search/hashtag?hashtag=streetwear&trim=true&region=US"
    );

    let hashtag_with_prefix = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::TikTokHashtag,
        "#streetwear",
        "US",
    )
    .unwrap();
    assert_eq!(
        hashtag_with_prefix,
        "https://api.scrapecreators.com/v1/tiktok/search/hashtag?hashtag=streetwear&trim=true&region=US"
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
fn scrape_platform_parser_normalizes_supported_inputs_and_rejects_unsupported() {
    assert_eq!(
        scrape_platform_from_str(" TikTok ", " KEYWORD ").unwrap(),
        ScrapePlatform::TikTokKeyword
    );
    assert_eq!(
        scrape_platform_from_str("tiktok", "hashtag").unwrap(),
        ScrapePlatform::TikTokHashtag
    );
    assert_eq!(
        scrape_platform_from_str("INSTAGRAM", "reels").unwrap(),
        ScrapePlatform::InstagramReels
    );

    let err = scrape_platform_from_str("youtube", "keyword").unwrap_err();
    assert_eq!(err.to_string(), "unsupported scrape platform");

    let err = scrape_platform_from_str("instagram", "keyword").unwrap_err();
    assert_eq!(err.to_string(), "unsupported scrape platform");
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
                    "cover": { "url_list": ["", "https://cdn.example/cover.jpg"] }
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
        items[0].source_url.as_deref(),
        Some("https://www.tiktok.com/@creator/video/725")
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
        items[0].source_url.as_deref(),
        Some("https://www.instagram.com/reel/ABC123/")
    );
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-02-03T04:05:06.000Z")
    );
}

#[test]
fn instagram_reels_normalizer_converts_numeric_taken_at_values() {
    let raw = serde_json::json!({
        "reels": [
            {
                "shortcode": "NUM123",
                "caption": "numeric timestamp",
                "thumbnail_url": "https://cdn.example/num.jpg",
                "like_count": 5,
                "owner": { "username": "numeric" },
                "taken_at": 1767225600
            },
            {
                "shortcode": "STR123",
                "caption": "numeric string timestamp",
                "thumbnail_url": "https://cdn.example/str.jpg",
                "like_count": 6,
                "owner": { "username": "numeric-string" },
                "taken_at": "1767225600"
            }
        ]
    });

    let items = normalize_instagram_reels_search(&raw);
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    assert_eq!(
        items[1].source_published_at.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
}
