//! The texts a locked layer shows (its padlock's tooltip, its context menu): why it is locked, one line per blocker
//! with what the support catalog says about it, e.g.
//! `mask generator "mg_dirt" — Replaceable: choose "replace" in the import review (MaterialCraft's dirt)`.

use materialcraft::paint::LayerLock;
use materialcraft_substance::translate::catalog::support_of;

/// "Locked (Substance 3D Painter import)".
pub fn heading(lock: &LayerLock) -> String {
    format!("Locked ({})", lock.source)
}

/// One line per blocker: reasons with the same key are one line (the first one's text, with how often it was met);
/// reasons without a key keep their own line. At most `max` lines; a last line says how many more there are.
pub fn reasons(lock: &LayerLock, max: usize) -> Vec<String> {
    // (key, first text, times met), in the order first met
    let mut grouped: Vec<(&str, &str, usize)> = Vec::new();
    for r in &lock.reasons {
        match grouped.iter_mut().find(|(k, _, _)| !r.key.is_empty() && *k == r.key) {
            Some(g) => g.2 += 1,
            None => grouped.push((&r.key, &r.text, 1)),
        }
    }
    let mut lines: Vec<String> = grouped.iter().take(max).map(|(key, text, n)| line(key, text, *n)).collect();
    if grouped.len() > max {
        lines.push(format!("… and {} more", grouped.len() - max));
    }
    lines
}

fn line(key: &str, text: &str, times: usize) -> String {
    let text = if text.is_empty() { key } else { text };
    let met = if times > 1 { format!(" (met {times} times)") } else { String::new() };
    match support_of(key) {
        Some(s) => format!("{text}{met} — {}: {}", s.label(), lower_first(&s.action())),
        None => format!("{text}{met}"),
    }
}

fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_lowercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft::paint::LockReason;

    #[test]
    fn reasons_group_by_key_with_the_catalog_status() {
        let lock = LayerLock::new(
            "import",
            vec![
                LockReason::new("strokes:painted-mask", "painted mask"),
                LockReason::new("brush-alpha:charcoal", "brush alpha \"charcoal\""),
                LockReason::new("strokes:painted-mask", "painted mask again"),
                LockReason::new("", "something else"),
                LockReason::new("", "and another"),
            ],
        );
        assert_eq!(heading(&lock), "Locked (import)");
        assert_eq!(
            reasons(&lock, 10),
            [
                "painted mask (met 2 times) — Replayable: import with stroke replay on",
                "brush alpha \"charcoal\" — Not supported yet: stays locked until MaterialCraft supports it",
                "something else",
                "and another",
            ]
        );
        assert_eq!(reasons(&lock, 1), ["painted mask (met 2 times) — Replayable: import with stroke replay on", "… and 3 more"]);
        assert!(reasons(&LayerLock::new("import", Vec::new()), 10).is_empty());
    }
}
