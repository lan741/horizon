//! Where the app may write, as iOS defines it.
//!
//! The desktop client keeps its files beside the executable and in `%APPDATA%`/`Application
//! Support`; an iOS app is sandboxed and may write only inside its container. `HOME` already
//! points at the container (`…/Data/Application/<UUID>`), so the same relative layout works:
//! `Documents/Horizon` for worlds and options, `Library/Caches/Horizon` for caches, `tmp` for
//! anything reproducible. `HORIZON_DATA_DIR` still wins, so a harness can point the app at a
//! folder imported through the Files app.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The container's home, or the current directory when the variable is absent (a host build).
pub fn home() -> PathBuf {
    home_of(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")))
}

fn home_of(env: Option<OsString>) -> PathBuf {
    env.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// The player's visible folder: what the Files app shows under "On My iPad › Horizon".
pub fn documents() -> PathBuf {
    documents_in(&home())
}

fn documents_in(home: &Path) -> PathBuf {
    home.join("Documents")
}

/// The game's data root: worlds, `options.txt`, resource and shader packs.
pub fn data_dir() -> PathBuf {
    data_dir_in(&home(), std::env::var_os("HORIZON_DATA_DIR"))
}

fn data_dir_in(home: &Path, override_dir: Option<OsString>) -> PathBuf {
    match override_dir {
        Some(dir) => PathBuf::from(dir),
        None => documents_in(home).join("Horizon"),
    }
}

/// Caches the system may delete under pressure: rebuilt meshes, downloaded packs.
pub fn cache_dir() -> PathBuf {
    home().join("Library").join("Caches").join("Horizon")
}

/// The temporary area: anything that must not survive a relaunch.
pub fn temp_dir() -> PathBuf {
    home().join("tmp")
}

/// Create the directories the client expects and return the data root.
pub fn prepare() -> std::io::Result<PathBuf> {
    let data = data_dir();
    for dir in [data.clone(), cache_dir(), temp_dir()] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_matches_the_desktop_one() {
        let home = Path::new("/var/mobile/Containers/Data/Application/ABC");
        assert_eq!(documents_in(home), home.join("Documents"));
        assert_eq!(
            data_dir_in(home, None),
            home.join("Documents").join("Horizon")
        );
    }

    #[test]
    fn the_override_wins_over_the_container() {
        let home = Path::new("/var/mobile");
        let chosen = data_dir_in(home, Some(OsString::from("/imported/minecraft")));
        assert_eq!(chosen, PathBuf::from("/imported/minecraft"));
    }

    #[test]
    fn a_host_without_a_home_still_has_a_root() {
        assert_eq!(home_of(None), PathBuf::from("."));
    }
}
