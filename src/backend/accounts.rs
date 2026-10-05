use chrono::{Duration, Utc};

use super::{normalize_base_url, AccountProfile, LauncherConfig, ServerAuthInformation};

pub const TOKEN_REFRESH_THRESHOLD: Duration = Duration::days(15);

pub fn upsert_account(cfg: &mut LauncherConfig, account: AccountProfile) -> String {
    let key = account_key(&account.auth_server, &account.user_id);

    if let Some(existing) = cfg.accounts.iter_mut().find(|a| {
        normalize_base_url(&a.auth_server) == normalize_base_url(&account.auth_server)
            && a.user_id == account.user_id
    }) {
        *existing = account;
    } else {
        cfg.accounts.push(account);
    }

    cfg.active_account_key = Some(key.clone());
    key
}

pub fn remove_account(cfg: &mut LauncherConfig, key: &str) {
    cfg.accounts.retain(|acc| account_key(&acc.auth_server, &acc.user_id) != key);
    if let Some(active) = &cfg.active_account_key {
        if active == key {
            cfg.active_account_key = None;
        }
    }
}

pub fn account_key(auth_server: &str, user_id: &str) -> String {
    format!("{}|{}", normalize_base_url(auth_server), user_id)
}

pub fn account_token_expired(account: &AccountProfile) -> bool {
    account.expire_time <= Utc::now()
}

pub fn account_token_needs_refresh(account: &AccountProfile) -> bool {
    account.expire_time <= Utc::now() + TOKEN_REFRESH_THRESHOLD
}

pub fn update_account_token(
    cfg: &mut LauncherConfig,
    key: &str,
    token: String,
    expire_time: chrono::DateTime<Utc>,
) -> bool {
    let Some(account) = cfg
        .accounts
        .iter_mut()
        .find(|acc| account_key(&acc.auth_server, &acc.user_id) == key)
    else {
        return false;
    };

    account.token = token;
    account.expire_time = expire_time;
    true
}

pub fn auth_mode_disabled(auth: &ServerAuthInformation) -> bool {
    match &auth.mode {
        serde_json::Value::String(s) => s.eq_ignore_ascii_case("disabled"),
        serde_json::Value::Number(n) => n.as_i64() == Some(2),
        _ => false,
    }
}

pub fn active_account_for_auth<'a>(cfg: &'a LauncherConfig, auth_url: &str) -> Option<&'a AccountProfile> {
    let normalized = normalize_base_url(auth_url);

    if let Some(active_key) = &cfg.active_account_key {
        if let Some(found) = cfg
            .accounts
            .iter()
            .find(|acc| account_key(&acc.auth_server, &acc.user_id) == *active_key)
        {
            return Some(found);
        }
    }

    cfg.accounts
        .iter()
        .find(|acc| normalize_base_url(&acc.auth_server) == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AccountProfile, DEFAULT_AUTH_SERVER};

    fn account(auth_server: &str, user_id: &str) -> AccountProfile {
        AccountProfile {
            auth_server: auth_server.to_string(),
            username: format!("user-{user_id}"),
            user_id: user_id.to_string(),
            token: "tok".to_string(),
            expire_time: chrono::Utc::now(),
        }
    }

    #[test]
    fn active_account_is_returned_even_if_auth_url_differs() {
        let mut cfg = LauncherConfig::default();
        cfg.accounts.push(account(DEFAULT_AUTH_SERVER, "1"));
        cfg.active_account_key = Some(account_key(DEFAULT_AUTH_SERVER, "1"));

        assert!(active_account_for_auth(&cfg, "https://auth.example.com/").is_some());
    }

    #[test]
    fn falls_back_to_matching_auth_server_without_active_key() {
        let mut cfg = LauncherConfig::default();
        cfg.accounts.push(account("https://auth.example.com/", "7"));
        cfg.active_account_key = None;

        assert!(active_account_for_auth(&cfg, "https://auth.example.com").is_some());
        assert!(active_account_for_auth(&cfg, "https://other.example.com/").is_none());
    }

    fn account_expiring_in(days: i64) -> AccountProfile {
        let mut acc = account(DEFAULT_AUTH_SERVER, "9");
        acc.expire_time = Utc::now() + Duration::days(days);
        acc
    }

    #[test]
    fn expired_token_is_detected() {
        let expired = account_expiring_in(-1);
        assert!(account_token_expired(&expired));
        assert!(account_token_needs_refresh(&expired));

        let fresh = account_expiring_in(60);
        assert!(!account_token_expired(&fresh));
        assert!(!account_token_needs_refresh(&fresh));
    }

    #[test]
    fn nearing_expiry_token_needs_refresh_but_is_not_expired() {
        let nearing = account_expiring_in(5);
        assert!(!account_token_expired(&nearing));
        assert!(account_token_needs_refresh(&nearing));
    }

    #[test]
    fn update_account_token_replaces_token_and_expiry() {
        let mut cfg = LauncherConfig::default();
        cfg.accounts.push(account(DEFAULT_AUTH_SERVER, "9"));
        let key = account_key(DEFAULT_AUTH_SERVER, "9");
        let new_expiry = Utc::now() + Duration::days(30);

        assert!(update_account_token(
            &mut cfg,
            &key,
            "new-token".to_string(),
            new_expiry
        ));
        let stored = &cfg.accounts[0];
        assert_eq!(stored.token, "new-token");
        assert_eq!(stored.expire_time, new_expiry);

        assert!(!update_account_token(
            &mut cfg,
            "https://nope.example.com/|0",
            "x".to_string(),
            new_expiry
        ));
    }
}
