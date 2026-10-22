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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Confirm,
    Deny,
}

impl PermissionPolicy {
    /// Daemon policy: read-only tools run, user actions wait for a person.
    /// System changes and destructive tools stay off. There is no polkit helper.
    pub fn daemon() -> Self {
        Self {
            allow_read_only: true,
            allow_user_action: true,
            allow_system_modification: false,
            allow_destructive: false,
        }
    }

    pub fn decide(&self, level: PermissionLevel, confirmation: ConfirmationPolicy) -> Decision {
        if matches!(confirmation, ConfirmationPolicy::Deny) {
            return Decision::Deny;
        }
        let enabled = match level {
            PermissionLevel::ReadOnly => self.allow_read_only,
            PermissionLevel::UserAction => self.allow_user_action,
            PermissionLevel::SystemModification => self.allow_system_modification,
            PermissionLevel::Destructive => self.allow_destructive,
        };
        if !enabled {
            return Decision::Deny;
        }
        if matches!(confirmation, ConfirmationPolicy::Always)
            && !matches!(level, PermissionLevel::ReadOnly)
        {
            return Decision::Confirm;
        }
        Decision::Allow
    }

    pub fn allows(&self, level: PermissionLevel, confirmation: ConfirmationPolicy) -> bool {
        !matches!(self.decide(level, confirmation), Decision::Deny)
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
    fn user_action_waits_for_confirmation() {
        let policy = PermissionPolicy::daemon();
        assert_eq!(
            policy.decide(PermissionLevel::UserAction, ConfirmationPolicy::Always),
            Decision::Confirm
        );
        assert_eq!(
            policy.decide(PermissionLevel::ReadOnly, ConfirmationPolicy::Always),
            Decision::Allow
        );
        let destructive = policy.decide(PermissionLevel::Destructive, ConfirmationPolicy::Always);
        assert_eq!(destructive, Decision::Deny);
    }

    #[test]
    fn deny_policy_blocks_even_read_only() {
        let policy = PermissionPolicy::default();
        assert!(!policy.allows(PermissionLevel::ReadOnly, ConfirmationPolicy::Deny));
    }
}
