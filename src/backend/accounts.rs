use super::{normalize_base_url, AccountProfile, LauncherConfig, ServerAuthInformation};

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
            if normalize_base_url(&found.auth_server) == normalized {
                return Some(found);
            }
        }
    }

    cfg.accounts
        .iter()
        .find(|acc| normalize_base_url(&acc.auth_server) == normalized)
}
