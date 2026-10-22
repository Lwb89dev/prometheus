use lda_common::{ConfirmationPolicy, PermissionLevel};

/// Switches checked in process, before any tool function runs.
///
/// Milestone 1 allows read-only tools only. Higher levels stay off until a
/// confirmation UI exists. A model request cannot flip these flags.
#[derive(Debug, Clone)]
pub struct PermissionPolicy {
    pub allow_read_only: bool,
    pub allow_user_action: bool,
    pub allow_system_modification: bool,
    pub allow_destructive: bool,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self {
            allow_read_only: true,
            allow_user_action: false,
            allow_system_modification: false,
            allow_destructive: false,
        }
    }
}

impl PermissionPolicy {
    pub fn allows(&self, level: PermissionLevel, confirmation: ConfirmationPolicy) -> bool {
        if matches!(confirmation, ConfirmationPolicy::Deny) {
            return false;
        }
        match level {
            PermissionLevel::ReadOnly => self.allow_read_only,
            PermissionLevel::UserAction => self.allow_user_action,
            PermissionLevel::SystemModification => self.allow_system_modification,
            PermissionLevel::Destructive => self.allow_destructive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_read_only() {
        let policy = PermissionPolicy::default();
        assert!(policy.allows(PermissionLevel::ReadOnly, ConfirmationPolicy::Never));
        assert!(!policy.allows(PermissionLevel::UserAction, ConfirmationPolicy::Always));
        assert!(!policy.allows(
            PermissionLevel::SystemModification,
            ConfirmationPolicy::Always
        ));
        assert!(!policy.allows(PermissionLevel::Destructive, ConfirmationPolicy::Always));
    }

    #[test]
    fn deny_policy_blocks_even_read_only() {
        let policy = PermissionPolicy::default();
        assert!(!policy.allows(PermissionLevel::ReadOnly, ConfirmationPolicy::Deny));
    }
}
