
use crate::backend::{
    build_content_database, download_content_entries, download_engine_module_for_engine_version,
    download_engine_zip_for_loader, ensure_loader_installed, launch_game_via_loader,
    normalize_base_url, ContentDbEntry, LoaderLaunchSpec, ServerInfo,
};

use super::LauncherApp;

impl LauncherApp {
    #[allow(dead_code)]
    pub(crate) fn try_launch_via_loader(
        &mut self,
        address: &str,
        info: &ServerInfo,
        engine_version: &str,
    ) -> anyhow::Result<bool> {
        let proxy = self.hub_options().proxy_url;

        self.set_progress(0.1, self.t("progress.loader", &[]));
        let loader = ensure_loader_installed(&self.paths, proxy.as_deref())?;

        self.set_progress(0.3, self.t("progress.engine", &[]));
        let (engine_zip, signature) =
            download_engine_zip_for_loader(&self.paths, engine_version, proxy.as_deref())?;

        let mut modules = Vec::new();
        if let Ok(module_dir) = download_engine_module_for_engine_version(
            &self.paths,
            "Robust.Client.WebView",
            engine_version,
            proxy.as_deref(),
        ) {
            modules.push(("Robust.Client.WebView".to_string(), module_dir.clone()));
            modules.push(("SpaceWizards.Sdl".to_string(), module_dir));
        }

        let content_db_path = self.paths.clients_dir.join("content.db");
        let mut content_db = content_db_path.clone();
        let mut content_version_id: i64 = 0;
        let mut have_content = false;

        if let Some(build) = &info.build {
            self.set_progress(0.5, self.t("progress.content", &[]));
            match download_content_entries(&self.paths, address, build, proxy.as_deref()) {
                Ok(Some((cache_dir, files, manifest_hash))) => {
                    self.set_progress(0.75, self.t("progress.content_db", &[]));
                    let entries: Vec<ContentDbEntry> = files
                        .into_iter()
                        .map(|f| ContentDbEntry {
                            path: f.path,
                            hash_hex: f.hash_hex,
                        })
                        .collect();
                    let db = build_content_database(
                        &content_db_path,
                        &cache_dir,
                        &entries,
                        &manifest_hash,
                        build.fork_id.as_deref(),
                        build.version.as_deref(),
                        engine_version,
                    )?;
                    content_db = db.path;
                    content_version_id = db.version_id;
                    have_content = true;
                }
                Ok(None) => {
                    return Ok(false);
                }
                Err(err) => return Err(err),
            }
        }

        if !have_content {
            return Ok(false);
        }

        self.set_progress(0.9, self.t("progress.launch", &[]));

        let mut build_cvars = Vec::new();
        if let Some(build) = &info.build {
            for (key, value) in [
                ("download_url", build.download_url.as_deref()),
                ("manifest_url", build.manifest_url.as_deref()),
                (
                    "manifest_download_url",
                    build.manifest_download_url.as_deref(),
                ),
                ("version", build.version.as_deref()),
                ("fork_id", build.fork_id.as_deref()),
                ("hash", build.hash.as_deref()),
                ("manifest_hash", build.manifest_hash.as_deref()),
                ("engine_version", build.engine_version.as_deref()),
            ] {
                if let Some(value) = value {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        build_cvars.push((key.to_string(), trimmed.to_string()));
                    }
                }
            }
        }

        let connect_uri = crate::backend::derive_connect_address(address, info)
            .unwrap_or_else(|_| address.to_string());

        let mut auth_token = None;
        let mut auth_userid = None;
        let mut auth_server = None;
        let mut auth_pubkey = None;
        let mut username = None;
        let disable_signing = signature.is_empty();
        if let Some(account) =
            crate::backend::active_account_for_auth(&self.cfg, &self.cfg.auth_server_url)
        {
            if !crate::backend::auth_mode_disabled(&info.auth) {
                username = Some(account.username.trim().to_string());
                auth_token = Some(account.token.trim().to_string());
                auth_userid = Some(account.user_id.trim().to_string());
                auth_server = Some(normalize_base_url(&account.auth_server));
                if let Some(pk) = &info.auth.public_key {
                    if !pk.trim().is_empty() {
                        auth_pubkey = Some(pk.trim().to_string());
                    }
                }
            }
        }

        let spec = LoaderLaunchSpec {
            engine_zip,
            engine_signature: signature,
            signing_key: loader.signing_key,
            loader_exe: loader.loader_exe,
            content_db,
            content_version_id,
            overlay_zip: None,
            modules,
            connect_uri: Some(connect_uri),
            connect_ss14_address: Some(address.to_string()),
            build_cvars,
            username,
            compat_mode: false,
            extra_args: Vec::new(),
            auth_token,
            auth_userid,
            auth_server,
            auth_pubkey,
            disable_signing,
        };

        match launch_game_via_loader(&spec) {
            Ok(mut child) => {
                let _ = child.wait();
                self.clear_progress();
            }
            Err(err) => return Err(err),
        }

        Ok(true)
    }
}

