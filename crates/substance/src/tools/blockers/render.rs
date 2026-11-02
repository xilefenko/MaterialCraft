//! The census as text: a markdown report (default), tab-separated tables, or JSON with everything.

use super::census::{Census, KeyTotal, Totals};
use super::project::ProjectBlockers;
use materialcraft::json::Value;
use materialcraft::paint::Channel;
use std::fmt::{self, Write};

/// Longest example text in a table cell.
const EXAMPLE_CHARS: usize = 80;

/// What the text tables show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Tab-separated values instead of markdown.
    pub tsv: bool,
    /// One row per project too.
    pub projects: bool,
    /// At most this many blockers in the ranked table (`None`: all).
    pub top: Option<usize>,
}

/// The census as markdown or TSV.
pub struct CensusText<'a> {
    pub census: &'a Census,
    pub options: RenderOptions,
}

impl fmt::Display for CensusText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        if self.options.tsv {
            tsv(self.census, self.options, &mut out)?;
        } else {
            markdown(self.census, self.options, &mut out)?;
        }
        f.write_str(&out)
    }
}

fn support_id(k: &KeyTotal) -> &'static str {
    k.support.map_or("unknown", |s| s.id())
}

fn support_label(k: &KeyTotal) -> &'static str {
    k.support.map_or("Unknown key", |s| s.label())
}

fn kind_id(k: &KeyTotal) -> &'static str {
    k.kind.map_or("?", |k| k.id())
}

/// `base color 12.5%, roughness 0.0%`.
fn coverage_text(channels: &[(Channel, f32)]) -> String {
    channels
        .iter()
        .map(|(c, share)| format!("{} {:.1}%", c.id().replace('_', " "), share * 100.0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A table cell: no pipes or line breaks, at most `max` characters.
fn cell(text: &str, max: usize) -> String {
    let clean: String = text
        .chars()
        .map(|c| match c {
            '|' => '/',
            '\n' | '\r' | '\t' => ' ',
            c => c,
        })
        .collect();
    if clean.chars().count() <= max {
        clean
    } else {
        let mut cut: String = clean.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        "–".to_string()
    } else {
        format!("{:.1}%", part as f64 * 100.0 / whole as f64)
    }
}

fn markdown(census: &Census, options: RenderOptions, out: &mut String) -> fmt::Result {
    let t = census.totals();
    writeln!(out, "# Blockers over {} projects\n", t.projects + t.failed)?;
    totals_markdown(census, &t, out)?;
    let ranked = census.ranked();
    let shown = options.top.unwrap_or(ranked.len()).min(ranked.len());
    writeln!(
        out,
        "\n## Blockers, by layers they lock alone ({shown} of {})\n\n\
         \"Alone\": locked layers it is the only blocker of, unlocked once it is supported. \"Visible\": of its \
         locked layers, the visible ones.\n",
        ranked.len()
    )?;
    writeln!(
        out,
        "| # | Blocker | Support | Replacement | Projects | Alone | Locked | Visible | Example |"
    )?;
    writeln!(out, "|---:|---|---|---|---:|---:|---:|---:|---|")?;
    for (i, k) in ranked.iter().take(shown).enumerate() {
        writeln!(
            out,
            "| {} | `{}` | {} | {} | {} | {} | {} | {} | {} |",
            i + 1,
            cell(&k.key, usize::MAX).replace('`', "'"),
            support_label(k),
            k.replacement().unwrap_or(""),
            k.projects,
            k.alone,
            k.layers,
            k.visible,
            cell(&k.example, EXAMPLE_CHARS)
        )?;
    }
    if !census.failures.is_empty() {
        writeln!(out, "\n## Failed imports\n")?;
        for fail in &census.failures {
            writeln!(out, "- `{}`: {}", fail.path.display(), cell(&fail.error, usize::MAX))?;
        }
    }
    if options.projects {
        writeln!(
            out,
            "\n## Projects\n\nCorrection: per channel, the mean over the sets of the share of texels where Painter's result is still needed.\n"
        )?;
        writeln!(
            out,
            "| Project | Sets | Layers | Locked | Visible locked | Blockers | Import s | Correction |"
        )?;
        writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---|")?;
        for p in &census.projects {
            writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {:.0} | {} |",
                cell(&p.path.display().to_string(), usize::MAX),
                p.sets,
                p.layers,
                p.locked(),
                visible_locked(p),
                p.summary.blockers.len(),
                p.seconds,
                coverage_text(&p.mean_coverage())
            )?;
        }
    }
    Ok(())
}

fn visible_locked(p: &ProjectBlockers) -> usize {
    p.summary.locked.iter().filter(|l| l.visible).count()
}

fn totals_markdown(census: &Census, t: &Totals, out: &mut String) -> fmt::Result {
    writeln!(out, "| | |\n|---|---:|")?;
    let rows: [(&str, String); 12] = [
        ("Projects imported", t.projects.to_string()),
        ("Projects failed", t.failed.to_string()),
        ("Projects with no locked layer", t.unlocked_projects.to_string()),
        ("Texture sets", t.sets.to_string()),
        ("Layers", t.layers.to_string()),
        (
            "Locked layers",
            format!("{} ({} of the layers)", t.locked, percent(t.locked, t.layers)),
        ),
        ("Visible locked layers", t.visible_locked.to_string()),
        ("Unlocked by choosing \"replace\"", t.by_replace.to_string()),
        ("Unlocked by replaying strokes", t.by_replay.to_string()),
        ("Unlocked by both choices", t.by_both.to_string()),
        (
            "Needing new support",
            format!(
                "{} ({} of the locked)",
                t.needs_support,
                percent(t.needs_support, t.locked)
            ),
        ),
        ("Distinct blockers", t.keys.to_string()),
    ];
    for (label, value) in rows {
        writeln!(out, "| {label} | {value} |")?;
    }
    writeln!(out, "| Wall time | {:.0} s |", census.seconds)?;
    let kinds = census.by_kind();
    if !kinds.is_empty() {
        writeln!(
            out,
            "\n| Kind | Locked layers | Locked by this kind alone |\n|---|---:|---:|"
        )?;
        for (kind, layers, alone) in kinds {
            writeln!(out, "| {} | {layers} | {alone} |", kind.label())?;
        }
    }
    Ok(())
}

fn tsv(census: &Census, options: RenderOptions, out: &mut String) -> fmt::Result {
    let tab = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    writeln!(
        out,
        "key\tkind\tsupport\treplacement\tprojects\talone\tlocked\tvisible\texample"
    )?;
    let ranked = census.ranked();
    for k in ranked.iter().take(options.top.unwrap_or(ranked.len())) {
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            tab(&k.key),
            kind_id(k),
            support_id(k),
            k.replacement().unwrap_or(""),
            k.projects,
            k.alone,
            k.layers,
            k.visible,
            tab(&k.example)
        )?;
    }
    if options.projects {
        writeln!(
            out,
            "\nproject\tsets\tlayers\tlocked\tvisible_locked\tblockers\timport_s\tcorrection"
        )?;
        for p in &census.projects {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{}",
                tab(&p.path.display().to_string()),
                p.sets,
                p.layers,
                p.locked(),
                visible_locked(p),
                p.summary.blockers.len(),
                p.seconds,
                coverage_text(&p.mean_coverage())
            )?;
        }
    }
    for fail in &census.failures {
        writeln!(
            out,
            "FAILED\t{}\t{}",
            tab(&fail.path.display().to_string()),
            tab(&fail.error)
        )?;
    }
    Ok(())
}

/// Everything as JSON: totals, the ranked blockers, every project with its locked layers, the failures.
pub fn census_json(census: &Census) -> Value {
    let t = census.totals();
    let totals = Value::object()
        .with("projects", t.projects)
        .with("failed", t.failed)
        .with("unlocked_projects", t.unlocked_projects)
        .with("sets", t.sets)
        .with("layers", t.layers)
        .with("locked", t.locked)
        .with("visible_locked", t.visible_locked)
        .with("by_replace", t.by_replace)
        .with("by_replay", t.by_replay)
        .with("by_both", t.by_both)
        .with("needs_support", t.needs_support)
        .with("keys", t.keys)
        .with("seconds", census.seconds);
    let kinds = census
        .by_kind()
        .into_iter()
        .map(|(kind, layers, alone)| {
            Value::object()
                .with("kind", kind.id())
                .with("layers", layers)
                .with("alone", alone)
        })
        .collect();
    let blockers = census.ranked().iter().map(key_json).collect();
    let projects = census.projects.iter().map(project_json).collect();
    let failures = census
        .failures
        .iter()
        .map(|f| {
            Value::object()
                .with("path", f.path.display().to_string())
                .with("error", f.error.as_str())
        })
        .collect();
    Value::object()
        .with("totals", totals)
        .with("kinds", Value::Array(kinds))
        .with("blockers", Value::Array(blockers))
        .with("projects", Value::Array(projects))
        .with("failures", Value::Array(failures))
}

fn key_json(k: &KeyTotal) -> Value {
    Value::object()
        .with("key", k.key.as_str())
        .with("kind", kind_id(k))
        .with("support", support_id(k))
        .with("support_label", support_label(k))
        .with("replacement", k.replacement().map_or(Value::Null, Value::from))
        .with("action", k.support.map_or(Value::Null, |s| Value::from(s.action())))
        .with("projects", k.projects)
        .with("alone", k.alone)
        .with("locked", k.layers)
        .with("visible", k.visible)
        .with("example", k.example.as_str())
}

fn channels_json(channels: &[(Channel, f32)]) -> Value {
    Value::Object(
        channels
            .iter()
            .map(|(c, share)| (c.id(), Value::from(f64::from(*share))))
            .collect(),
    )
}

fn project_json(p: &ProjectBlockers) -> Value {
    let blockers = p
        .summary
        .blockers
        .iter()
        .map(|b| {
            Value::object()
                .with("key", b.key.as_str())
                .with("locked", b.layers.len())
                .with("alone", b.only)
        })
        .collect();
    let locked = p
        .summary
        .locked
        .iter()
        .map(|l| {
            let keys = l.keys.iter().map(|k| Value::from(k.as_str())).collect();
            Value::object()
                .with("set", l.set_name.as_str())
                .with("layer", l.path.as_str())
                .with("visible", l.visible)
                .with("keys", Value::Array(keys))
        })
        .collect();
    let coverage = p
        .coverage
        .iter()
        .map(|s| {
            Value::object()
                .with("set", s.set.as_str())
                .with("channels", channels_json(&s.channels))
        })
        .collect();
    Value::object()
        .with("path", p.path.display().to_string())
        .with("sets", p.sets)
        .with("layers", p.layers)
        .with("locked", p.locked())
        .with("seconds", p.seconds)
        .with("correction_mean", channels_json(&p.mean_coverage()))
        .with("correction", Value::Array(coverage))
        .with("blockers", Value::Array(blockers))
        .with("locked_layers", Value::Array(locked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::blockers::census::Failure;
    use crate::translate::blocker_summary::{BlockerSummary, LockedLayer};
    use std::path::PathBuf;

    fn census() -> Census {
        let locked = vec![LockedLayer {
            set: "s".into(),
            set_name: "S".into(),
            layer: "1".into(),
            path: "Dirt".into(),
            visible: true,
            keys: vec!["substance:a|b".into()],
        }];
        Census {
            projects: vec![ProjectBlockers {
                path: PathBuf::from("x.spp"),
                sets: 1,
                layers: 3,
                summary: BlockerSummary::from_locked(locked),
                coverage: Vec::new(),
                seconds: 2.0,
            }],
            failures: vec![Failure {
                path: PathBuf::from("y.spp"),
                error: "bad\nfile".into(),
            }],
            seconds: 3.0,
        }
    }

    #[test]
    fn markdown_has_totals_ranked_rows_and_failures_without_breaking_cells() {
        let options = RenderOptions {
            tsv: false,
            projects: true,
            top: None,
        };
        let text = CensusText {
            census: &census(),
            options,
        }
        .to_string();
        assert!(text.contains("| Projects imported | 1 |"), "{text}");
        assert!(text.contains("| 1 | `substance:a/b` | Not supported yet |"), "{text}");
        assert!(text.contains("- `y.spp`: bad file"), "{text}");
        assert!(text.contains("| x.spp | 1 | 3 | 1 | 1 | 1 | 2 |"), "{text}");
    }

    #[test]
    fn tsv_and_json_carry_the_same_blockers() {
        let c = census();
        let options = RenderOptions {
            tsv: true,
            projects: false,
            top: Some(5),
        };
        let text = CensusText { census: &c, options }.to_string();
        assert!(
            text.lines()
                .any(|l| l.starts_with("substance:a|b\tsubstance\tunsupported\t\t1\t1\t1\t1")),
            "{text}"
        );
        let json = census_json(&c);
        assert_eq!(json["blockers"][0]["key"].as_str(), Some("substance:a|b"));
        assert_eq!(json["totals"]["failed"].as_usize(), Some(1));
        assert_eq!(json["projects"][0]["locked_layers"][0]["layer"].as_str(), Some("Dirt"));
    }

    #[test]
    fn long_cells_are_cut() {
        assert_eq!(cell("abcdef", 4), "abc…");
        assert_eq!(cell("a|b", 10), "a/b");
    }
}
