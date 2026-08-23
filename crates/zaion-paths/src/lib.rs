use std::ffi::OsString;
use std::path::PathBuf;

pub const ENV_ZAION_HOME: &str = "ZAION_HOME";
pub const ENV_ZAION_DATA_DIR: &str = "ZAION_DATA_DIR";
pub const DEFAULT_ZAION_DIR: &str = ".zaion";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub source: String,
}

impl ResolvedPath {
    fn new(path: PathBuf, source: impl Into<String>) -> Self {
        Self {
            path,
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZaionPaths {
    pub home: ResolvedPath,
    pub data_dir: ResolvedPath,
}

impl ZaionPaths {
    pub fn from_env() -> Self {
        let home = zaion_home_with_source();
        let data_dir = zaion_data_dir_with_source(&home);
        Self { home, data_dir }
    }

    pub fn config_path(&self) -> PathBuf {
        self.home.path.join("config.toml")
    }

    pub fn channels_path(&self) -> PathBuf {
        self.home.path.join("channels.toml")
    }

    pub fn webhooks_path(&self) -> PathBuf {
        self.home.path.join("webhooks.toml")
    }

    pub fn mcp_path(&self) -> PathBuf {
        self.home.path.join("mcp.toml")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.home.path.join("profiles")
    }

    pub fn profiles_index_path(&self) -> PathBuf {
        self.profiles_dir().join("profiles.toml")
    }

    pub fn honcho_path(&self) -> PathBuf {
        self.home.path.join("honcho.toml")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.home.path.join("skills")
    }

    pub fn display_config_path(&self) -> PathBuf {
        self.home.path.join("display.toml")
    }

    pub fn checkpoint_root(&self) -> PathBuf {
        self.data_dir.path.join("checkpoints")
    }
}

pub fn paths() -> ZaionPaths {
    ZaionPaths::from_env()
}

pub fn user_home_dir() -> PathBuf {
    user_home_dir_with_source().path
}

pub fn zaion_home() -> PathBuf {
    zaion_home_with_source().path
}

pub fn data_dir() -> PathBuf {
    paths().data_dir.path
}

pub fn config_path() -> PathBuf {
    paths().config_path()
}

pub fn channels_path() -> PathBuf {
    paths().channels_path()
}

pub fn webhooks_path() -> PathBuf {
    paths().webhooks_path()
}

pub fn mcp_path() -> PathBuf {
    paths().mcp_path()
}

pub fn profiles_dir() -> PathBuf {
    paths().profiles_dir()
}

pub fn profiles_index_path() -> PathBuf {
    paths().profiles_index_path()
}

pub fn honcho_path() -> PathBuf {
    paths().honcho_path()
}

pub fn skills_dir() -> PathBuf {
    paths().skills_dir()
}

pub fn display_config_path() -> PathBuf {
    paths().display_config_path()
}

pub fn checkpoint_root() -> PathBuf {
    paths().checkpoint_root()
}

pub fn user_home_dir_with_source() -> ResolvedPath {
    if let Some(path) = env_path("HOME") {
        return ResolvedPath::new(path, "HOME");
    }
    if let Some(path) = env_path("USERPROFILE") {
        return ResolvedPath::new(path, "USERPROFILE");
    }
    ResolvedPath::new(PathBuf::from("."), "current directory fallback")
}

pub fn zaion_home_with_source() -> ResolvedPath {
    if let Some(path) = env_path(ENV_ZAION_HOME) {
        return ResolvedPath::new(path, ENV_ZAION_HOME);
    }

    let home = user_home_dir_with_source();
    ResolvedPath::new(
        home.path.join(DEFAULT_ZAION_DIR),
        format!("{}/{}", home.source, DEFAULT_ZAION_DIR),
    )
}

fn zaion_data_dir_with_source(home: &ResolvedPath) -> ResolvedPath {
    if let Some(path) = env_path(ENV_ZAION_DATA_DIR) {
        return ResolvedPath::new(path, ENV_ZAION_DATA_DIR);
    }

    ResolvedPath::new(home.path.clone(), format!("{} default data", home.source))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !is_blank(value))
        .map(PathBuf::from)
}

fn is_blank(value: &OsString) -> bool {
    value.to_string_lossy().trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        home: Option<OsString>,
        userprofile: Option<OsString>,
        zaion_home: Option<OsString>,
        zaion_data_dir: Option<OsString>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                home: std::env::var_os("HOME"),
                userprofile: std::env::var_os("USERPROFILE"),
                zaion_home: std::env::var_os(ENV_ZAION_HOME),
                zaion_data_dir: std::env::var_os(ENV_ZAION_DATA_DIR),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore_env("HOME", self.home.take());
            restore_env("USERPROFILE", self.userprofile.take());
            restore_env(ENV_ZAION_HOME, self.zaion_home.take());
            restore_env(ENV_ZAION_DATA_DIR, self.zaion_data_dir.take());
        }
    }

    fn restore_env(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn zaion_home_overrides_user_home() {
        let _test_guard = env_lock();
        let _guard = EnvGuard::capture();
        std::env::set_var("HOME", "/tmp/user-home");
        std::env::remove_var("USERPROFILE");
        std::env::set_var(ENV_ZAION_HOME, "/tmp/zaion-home");
        std::env::remove_var(ENV_ZAION_DATA_DIR);

        let paths = ZaionPaths::from_env();

        assert_eq!(paths.home.path, PathBuf::from("/tmp/zaion-home"));
        assert_eq!(paths.home.source, ENV_ZAION_HOME);
        assert_eq!(paths.data_dir.path, PathBuf::from("/tmp/zaion-home"));
    }

    #[test]
    fn data_dir_can_be_advanced_override() {
        let _test_guard = env_lock();
        let _guard = EnvGuard::capture();
        std::env::set_var(ENV_ZAION_HOME, "/tmp/zaion-home");
        std::env::set_var(ENV_ZAION_DATA_DIR, "/tmp/zaion-data");

        let paths = ZaionPaths::from_env();

        assert_eq!(
            paths.config_path(),
            PathBuf::from("/tmp/zaion-home/config.toml")
        );
        assert_eq!(paths.data_dir.path, PathBuf::from("/tmp/zaion-data"));
        assert_eq!(paths.data_dir.source, ENV_ZAION_DATA_DIR);
    }

    #[test]
    fn default_home_uses_user_home_dot_zaion() {
        let _test_guard = env_lock();
        let _guard = EnvGuard::capture();
        std::env::set_var("HOME", "/tmp/user-home");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var(ENV_ZAION_HOME);
        std::env::remove_var(ENV_ZAION_DATA_DIR);

        let paths = ZaionPaths::from_env();

        assert_eq!(paths.home.path, PathBuf::from("/tmp/user-home/.zaion"));
        assert_eq!(paths.data_dir.path, PathBuf::from("/tmp/user-home/.zaion"));
    }


    #[test]
    fn blank_zaion_home_falls_back_to_default() {
        let _test_guard = env_lock();
        let _guard = EnvGuard::capture();
        std::env::set_var("HOME", "/tmp/user-home");
        std::env::remove_var("USERPROFILE");
        std::env::set_var(ENV_ZAION_HOME, "   ");
        std::env::remove_var(ENV_ZAION_DATA_DIR);

        let paths = ZaionPaths::from_env();

        assert_eq!(paths.home.path, PathBuf::from("/tmp/user-home/.zaion"));
        assert!(!paths.home.path.as_os_str().is_empty());
    }

    #[test]
    fn derived_paths_stay_under_home() {
        let _test_guard = env_lock();
        let _guard = EnvGuard::capture();
        std::env::set_var(ENV_ZAION_HOME, "/tmp/zaion-home");
        std::env::remove_var(ENV_ZAION_DATA_DIR);

        let paths = ZaionPaths::from_env();

        for derived in [
            paths.config_path(),
            paths.channels_path(),
            paths.webhooks_path(),
            paths.mcp_path(),
            paths.profiles_dir(),
            paths.skills_dir(),
            paths.checkpoint_root(),
        ] {
            assert!(
                derived.starts_with(&paths.home.path),
                "derived path escapes home: {}",
                derived.display()
            );
        }
    }

}
