use sha2::{Digest, Sha256};

pub fn clone_upload_key(user_id: &str, display_name: &str, file_hashes: &[String]) -> String {
    let normalized_display_name = normalize_display_name(display_name);
    let mut sorted_hashes = file_hashes.to_vec();
    sorted_hashes.sort();

    let mut hasher = Sha256::new();
    update_part(&mut hasher, user_id);
    update_part(&mut hasher, &normalized_display_name);
    for file_hash in sorted_hashes {
        update_part(&mut hasher, &file_hash);
    }

    format!(
        "clone_upload:{}:{}",
        user_id,
        hex::encode(hasher.finalize())
    )
}

fn normalize_display_name(display_name: &str) -> String {
    display_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn update_part(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}
