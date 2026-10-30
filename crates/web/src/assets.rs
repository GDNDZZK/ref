//! The embedded web frontend (see build.rs).

/// One file of `web/dist`, with the pre-compressed variants that are smaller than the original.
pub struct Asset {
    /// URL path, e.g. `/assets/app.js`.
    pub path: &'static str,
    pub mime: &'static str,
    pub raw: &'static [u8],
    pub br: Option<&'static [u8]>,
    pub gz: Option<&'static [u8]>,
}

static ASSETS: &[Asset] = include!(concat!(env!("OUT_DIR"), "/assets.rs"));

pub fn find(path: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.path == path)
}
