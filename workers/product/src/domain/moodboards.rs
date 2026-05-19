use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoodboardSeed {
    pub slug: String,
    pub title: String,
    pub vibe_summary: String,
    pub search_queries: Vec<String>,
}

pub fn default_moodboards() -> Vec<MoodboardSeed> {
    vec![
        moodboard_seed("warm-ambient", "Warm ambient", "Soft tungsten warmth, calm rooms, skin glow, and relaxed editorial framing."),
        moodboard_seed("y2k-studio", "Y2K studio", "Glossy flash studio portraits, chrome accents, playful styling, and polished social poses."),
        moodboard_seed("swag-era", "Swag era", "Bold accessories, confident casual poses, bright flash, and early social-era outfit energy."),
        moodboard_seed("theatrical-light", "Theatrical light", "Dramatic spotlights, sculpted shadows, stage color, and cinematic portrait contrast."),
        moodboard_seed("y2k-street", "Y2K street", "Street snapshots, low-rise layers, compact cameras, and saturated city color."),
        moodboard_seed("flash-editorial", "Flash editorial", "Direct flash, crisp styling, strong makeup, studio walls, and magazine energy."),
        moodboard_seed("old-smartphone", "Old smartphone", "Soft phone-camera grain, imperfect framing, casual mirror shots, and nostalgic texture."),
        moodboard_seed("street-photography", "Street photography", "Candid sidewalks, real city motion, natural outfits, and documentary framing."),
        moodboard_seed("asian-nostalgia", "Asian nostalgia", "Warm city evenings, intimate cafes, retro interiors, and soft nostalgic styling."),
        moodboard_seed("retro-bw", "Retro BW", "High-grain black and white portraits, strong contrast, and vintage editorial attitude."),
        moodboard_seed("subtle-flash", "Subtle flash", "Low-key direct flash, soft shadows, realistic skin, and understated nightlife polish."),
        moodboard_seed("surreal-solarization", "Surreal solarization", "Experimental color inversions, glowing edges, and dreamlike fashion portrait effects."),
        moodboard_seed("digital-camera", "Digital camera", "Compact-camera sharpness, glossy highlights, dated timestamps, and candid creator snaps."),
        moodboard_seed("siren", "Siren", "Sleek glam, moody nightlife, sharp silhouettes, and magnetic editorial confidence."),
        moodboard_seed("mystique-city", "Mystique city", "Dark urban atmosphere, reflective streets, elegant styling, and secretive cinematic light."),
        moodboard_seed("candy-pop", "Candy pop", "Bright color blocking, playful beauty details, glossy styling, and upbeat studio energy."),
        moodboard_seed("double-exposure", "Double exposure", "Layered portraits, ghosted motion, city overlays, and experimental photographic texture."),
        moodboard_seed("2000s-band", "2000s band", "Indie band flash, backstage styling, instrument-room texture, and casual group-photo attitude."),
        moodboard_seed("frutiger-aero", "Frutiger aero", "Glossy blue-green futurism, water reflections, glassy surfaces, and optimistic digital polish."),
        moodboard_seed("drain", "Drain", "Washed-out cool tones, underground styling, stark flash, and melancholic street energy."),
        moodboard_seed("extraterrestrial", "Extraterrestrial", "Alien color casts, metallic styling, unusual poses, and otherworldly editorial light."),
        moodboard_seed("nature-light", "Nature light", "Clean daylight, greenery, soft skin tones, and organic outdoor portrait calm."),
        moodboard_seed("editorial-street-style", "Editorial street style", "Runway-informed street outfits, confident full-body framing, and crisp city polish."),
        moodboard_seed("new-indie", "New Indie", "Modern indie styling, casual interiors, soft flash, and intimate music-scene energy."),
        moodboard_seed("underwater", "Underwater", "Blue cast light, floating fabric, softened movement, and submerged dreamlike portraits."),
        moodboard_seed("80s-horror", "80s horror", "Hard colored light, suspenseful shadows, retro styling, and cinematic genre tension."),
        moodboard_seed("disposable-camera", "Disposable camera", "Warm film grain, party flash, imperfect framing, and spontaneous memory-card texture."),
        moodboard_seed("neutral-pastel-film", "Neutral pastel film", "Soft muted pastels, low contrast, delicate grain, and gentle daylight portraits."),
        moodboard_seed("warm-vivid-film", "Warm vivid film", "Saturated warm film color, sunny skin tones, and energetic analog contrast."),
        moodboard_seed("bw-film", "BW film", "Classic black and white film grain, silver highlights, and timeless portrait contrast."),
        moodboard_seed("warm-contrast-film", "Warm contrast film", "Golden highlights, deep shadows, rich analog color, and confident editorial warmth."),
        moodboard_seed("muted-cool-film", "Muted cool film", "Cool gray-green film tones, restrained contrast, and quiet cinematic mood."),
    ]
}

pub fn moodboard_seed(slug: &str, title: &str, vibe_summary: &str) -> MoodboardSeed {
    let search_base = title.to_ascii_lowercase();
    MoodboardSeed {
        slug: slug.to_string(),
        title: title.to_string(),
        vibe_summary: vibe_summary.to_string(),
        search_queries: vec![
            format!("{search_base} creator aesthetic"),
            format!("{search_base} fashion portrait"),
            format!("{search_base} social photo style"),
        ],
    }
}

pub fn deterministic_user_moodboard_id(user_id: &str, slug: &str) -> String {
    let mut hasher = Sha256::new();
    update_hash_part(&mut hasher, user_id.trim());
    update_hash_part(&mut hasher, slug.trim());
    format!("moodboard_{}", &hex::encode(hasher.finalize())[..24])
}

pub fn selected_moodboard_hash(slugs: &[String]) -> String {
    let mut normalized = slugs
        .iter()
        .map(|slug| slug.trim().to_ascii_lowercase())
        .filter(|slug| !slug.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    let payload = serde_json::to_string(&normalized).unwrap_or_else(|_| "[]".to_string());
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn selected_moodboard_count_is_valid(count: usize) -> bool {
    (1..=10).contains(&count)
}

pub fn active_selected_slugs(rows: Vec<(String, bool, String)>) -> Vec<String> {
    rows.into_iter()
        .filter_map(|(slug, selected, status)| {
            (selected && status.trim().eq_ignore_ascii_case("active"))
                .then(|| slug.trim().to_ascii_lowercase())
        })
        .filter(|slug| !slug.is_empty())
        .collect()
}

fn update_hash_part(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}
