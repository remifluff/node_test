use std::path::PathBuf;

pub fn install() {}

pub fn event(_msg: impl AsRef<str>) {}

pub fn log_path() -> PathBuf {
    PathBuf::from("crash.log")
}
