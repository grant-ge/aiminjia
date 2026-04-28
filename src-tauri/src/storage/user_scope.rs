use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserScope {
    pub tenant_id: i64,
    pub user_id: i64,
}

impl UserScope {
    pub fn new(tenant_id: i64, user_id: i64) -> Self {
        Self { tenant_id, user_id }
    }

    pub fn key(&self) -> String {
        format!("t_{}__u_{}", self.tenant_id, self.user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::UserScope;

    #[test]
    fn key_format_stable() {
        let scope = UserScope::new(1, 2);

        assert_eq!(scope.key(), "t_1__u_2");
    }

    #[test]
    fn key_format_large_ids() {
        let scope = UserScope::new(123456, 789012);

        assert_eq!(scope.key(), "t_123456__u_789012");
    }
}
