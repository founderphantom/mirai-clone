pub fn slugify_handle(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    let mut slug = String::new();
    let mut previous_dash = false;

    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.len() > 48 {
        slug.truncate(48);
        while slug.ends_with('-') {
            slug.pop();
        }
    }

    if slug.is_empty() {
        "my-soul".to_string()
    } else {
        slug
    }
}

pub fn handle_with_suffix(base: &str, suffix: u32) -> String {
    if suffix <= 1 {
        return base.chars().take(48).collect();
    }

    let suffix_text = format!("-{suffix}");
    let prefix_len = 48usize.saturating_sub(suffix_text.len());
    let mut prefix = base.chars().take(prefix_len).collect::<String>();
    while prefix.ends_with('-') {
        prefix.pop();
    }

    if prefix.is_empty() {
        format!("my-soul{suffix_text}")
    } else {
        format!("{prefix}{suffix_text}")
    }
}
