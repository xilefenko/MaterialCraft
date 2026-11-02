//! Entries of `/texture-cache`: the flattened layer-stack result per texture set and channel.
//! Paths are `/texture-cache/<set>/<n>` (2019) or `/texture-cache/<set>/<tile>/<n>` (2020 on), where `<n>` is
//! a single channel bit (see the internal docs for the table).

use super::layout::Layout;
use super::paths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntryRef {
    pub path: String,
    /// Texture set id.
    pub set: String,
    /// Middle level added in 2020; the same constant in every file seen so far.
    pub tile: Option<String>,
    /// Channel bit, e.g. 1 = base colour, 2 = height.
    pub channel_bit: u64,
    pub len: u64,
}

impl CacheEntryRef {
    /// Numeric leaf datasets below `/texture-cache`; `header` and `list` are skipped.
    pub fn all_in(layout: &Layout) -> Vec<CacheEntryRef> {
        layout
            .datasets_under(paths::TEXTURE_CACHE)
            .filter_map(|d| Self::from_path(d.path, d.len))
            .collect()
    }

    fn from_path(path: &str, len: u64) -> Option<CacheEntryRef> {
        let rest = path.strip_prefix(paths::TEXTURE_CACHE)?.strip_prefix('/')?;
        let parts: Vec<&str> = rest.split('/').collect();
        let (set, tile, leaf) = match parts.as_slice() {
            [set, leaf] => (set, None, leaf),
            [set, tile, leaf] => (set, Some(tile.to_string()), leaf),
            _ => return None,
        };
        let channel_bit = leaf.parse().ok()?;
        Some(CacheEntryRef {
            path: path.to_string(),
            set: set.to_string(),
            tile,
            channel_bit,
            len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_numeric_leaves_count() {
        assert_eq!(
            CacheEntryRef::from_path("/texture-cache/7/38/128", 4).map(|e| e.channel_bit),
            Some(128)
        );
        assert_eq!(
            CacheEntryRef::from_path("/texture-cache/7/2", 4).map(|e| e.tile),
            Some(None)
        );
        assert!(CacheEntryRef::from_path("/texture-cache/7/38/list", 4).is_none());
        assert!(CacheEntryRef::from_path("/texture-cache/header", 4).is_none());
    }
}
