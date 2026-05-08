#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    pub max_active_clones: u32,
}

pub fn can_create_clone(
    entitlements: &Entitlements,
    active_clone_count: u32,
) -> Result<(), &'static str> {
    if active_clone_count >= entitlements.max_active_clones {
        Err("clone_limit_reached")
    } else {
        Ok(())
    }
}
