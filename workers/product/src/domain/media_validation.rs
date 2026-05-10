#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceCountError {
    TooFew,
    TooMany,
}

pub fn validate_reference_count(count: usize) -> Result<(), ReferenceCountError> {
    match count {
        0..=4 => Err(ReferenceCountError::TooFew),
        5..=20 => Ok(()),
        _ => Err(ReferenceCountError::TooMany),
    }
}

pub fn is_supported_reference_content_type(content_type: &str) -> bool {
    let normalized = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/heic" | "image/heif"
    )
}
