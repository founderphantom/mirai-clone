use mirai_product_worker::domain::entitlements::{can_create_clone, Entitlements};
use mirai_product_worker::domain::idempotency::clone_upload_key;
use mirai_product_worker::domain::media_validation::{
    is_supported_reference_content_type, validate_reference_count, ReferenceCountError,
};
use mirai_product_worker::domain::status::{can_transition_soul_status, SoulStatus};
use mirai_product_worker::services::accounts::{
    account_checkout_enabled, account_entitlement_snapshot, account_portal_enabled,
    account_usage_limits, VerifiedIdentity,
};
use mirai_product_worker::services::media::{media_storage_key, normalize_extension};
use serde_json::json;

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
fn normalize_extension_uses_content_type() {
    assert_eq!(normalize_extension("image/jpeg"), "jpg");
    assert_eq!(normalize_extension("image/png"), "png");
    assert_eq!(normalize_extension("image/webp"), "webp");
    assert_eq!(normalize_extension("image/heic"), "heic");
}
