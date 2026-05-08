#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    pub max_active_clones: u32,
}

pub const FREE_MAX_ACTIVE_CLONES: u32 = 1;
pub const PAID_MAX_ACTIVE_CLONES: u32 = 5;

impl Entitlements {
    pub const fn free() -> Self {
        Self {
            max_active_clones: FREE_MAX_ACTIVE_CLONES,
        }
    }

    pub const fn paid() -> Self {
        Self {
            max_active_clones: PAID_MAX_ACTIVE_CLONES,
        }
    }
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
