use mirai_product_worker::domain::entitlements::{can_create_clone, Entitlements};
use mirai_product_worker::domain::idempotency::clone_upload_key;
use mirai_product_worker::domain::media_validation::{
    validate_reference_count, ReferenceCountError,
};
use mirai_product_worker::domain::status::{can_transition_soul_status, SoulStatus};

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
