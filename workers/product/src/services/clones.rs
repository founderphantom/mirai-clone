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
