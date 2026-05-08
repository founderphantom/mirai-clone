pub fn media_storage_key(
    user_id: &str,
    clone_id: &str,
    media_id: &str,
    content_type: &str,
) -> String {
    format!(
        "users/{}/clones/{}/{}.{}",
        safe_segment(user_id),
        safe_segment(clone_id),
        safe_segment(media_id),
        normalize_extension(content_type)
    )
}

pub fn normalize_extension(content_type: &str) -> &'static str {
    let normalized = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" | "image/heif" => "heic",
        _ => "jpg",
    }
}

pub fn safe_segment(value: &str) -> String {
    let normalized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .take(96)
        .collect();

    if normalized.is_empty() || normalized == "." || normalized == ".." {
        "segment".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_content_type_params() {
        assert_eq!(normalize_extension("image/png; charset=binary"), "png");
    }

    #[test]
    fn safe_segments_are_deterministic_and_capped() {
        let value = "a/b:c".repeat(30);
        let segment = safe_segment(&value);
        assert_eq!(segment.len(), 96);
        assert!(segment
            .chars()
            .all(|ch| { ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') }));
    }

    #[test]
    fn safe_segments_have_fallbacks_for_empty_or_dot_only_values() {
        assert_eq!(safe_segment(""), "segment");
        assert_eq!(safe_segment("."), "segment");
        assert_eq!(safe_segment(".."), "segment");
    }
}
