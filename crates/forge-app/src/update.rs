//! "New version available" check against GitHub Releases. Fails silently: no network, no message.
const LATEST: &str = "https://api.github.com/repos/DhakadG/husky-forge/releases/latest";
pub const RELEASES: &str = "https://github.com/DhakadG/husky-forge/releases";

/// Returns the newer tag if one exists.
pub fn newer_release() -> Option<String> {
    let body: serde_json::Value = ureq::get(LATEST).header("User-Agent", "husky-forge").call().ok()?.body_mut().read_json().ok()?;
    let tag = body.get("tag_name")?.as_str()?.to_string();
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    (parse(&tag)? > parse(env!("CARGO_PKG_VERSION"))?).then_some(tag)
}
