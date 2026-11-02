//! [`Computability`]: how far one part of a stack (a layer's content for a channel, a mask, a blend mode,
//! a whole channel) can be reproduced from the document alone, with the reasons when it cannot. What makes a part
//! not computable is always a typed [`Blocker`], so locks, reports and `mcsub blockers` can group by its key.

use crate::translate::blockers::Blocker;

/// At most this many distinct reasons are kept per part (the report stays readable).
const MAX_REASONS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Computability {
    /// Reproduced as Painter computes it.
    #[default]
    Exact,
    /// Exact apart from Substance procedurals the user chose to replace with MaterialCraft's own generators
    /// (computable and editable, measured but not exact).
    Replaced(Vec<String>),
    /// Reproduced with a stand-in (a guessed blend mode, a resampled bitmap, …).
    Approximated(Vec<String>),
    /// Needs something we cannot evaluate yet (procedurals, strokes, …), by blocker.
    NotComputable(Vec<Blocker>),
}

impl Computability {
    pub fn approximated(reason: impl Into<String>) -> Computability {
        Computability::Approximated(vec![reason.into()])
    }

    pub fn not_computable(blocker: Blocker) -> Computability {
        Computability::NotComputable(vec![blocker])
    }

    pub fn replaced(reason: impl Into<String>) -> Computability {
        Computability::Replaced(vec![reason.into()])
    }

    /// Exact, or exact apart from chosen replacements.
    pub fn is_exact_or_replaced(&self) -> bool {
        matches!(self, Computability::Exact | Computability::Replaced(_))
    }

    pub fn is_exact(&self) -> bool {
        matches!(self, Computability::Exact)
    }

    /// Exact or approximated: there are pixels to show.
    pub fn is_computable(&self) -> bool {
        !matches!(self, Computability::NotComputable(_))
    }

    /// The reasons as text (a not computable part's are its blockers' texts).
    pub fn reasons(&self) -> Vec<String> {
        match self {
            Computability::Exact => Vec::new(),
            Computability::Replaced(r) | Computability::Approximated(r) => r.clone(),
            Computability::NotComputable(b) => b.iter().map(|b| b.text.clone()).collect(),
        }
    }

    /// What makes it not computable (empty when it is computable).
    pub fn blockers(&self) -> &[Blocker] {
        match self {
            Computability::NotComputable(b) => b,
            _ => &[],
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Computability::Exact => 0,
            Computability::Replaced(_) => 1,
            Computability::Approximated(_) => 2,
            Computability::NotComputable(_) => 3,
        }
    }

    /// Combines with another part: the worse state wins, reasons of that state are merged.
    pub fn and(&mut self, other: &Computability) {
        if other.rank() > self.rank() {
            *self = match other {
                Computability::Replaced(_) => Computability::Replaced(Vec::new()),
                Computability::Approximated(_) => Computability::Approximated(Vec::new()),
                Computability::NotComputable(_) => Computability::NotComputable(Vec::new()),
                Computability::Exact => Computability::Exact,
            };
        }
        if other.rank() != self.rank() {
            return;
        }
        match self {
            Computability::Replaced(r) | Computability::Approximated(r) => {
                for reason in other.reasons() {
                    if !r.contains(&reason) && r.len() < MAX_REASONS {
                        r.push(reason);
                    }
                }
            }
            Computability::NotComputable(b) => {
                for blocker in other.blockers() {
                    if !b.contains(blocker) && b.len() < MAX_REASONS {
                        b.push(blocker.clone());
                    }
                }
            }
            Computability::Exact => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_state_wins_and_reasons_merge() {
        let mut c = Computability::Exact;
        c.and(&Computability::approximated("guess"));
        assert_eq!(c, Computability::approximated("guess"));
        let strokes = Blocker::feature("strokes", "strokes");
        c.and(&Computability::not_computable(strokes.clone()));
        c.and(&Computability::approximated("other guess"));
        c.and(&Computability::not_computable(strokes));
        c.and(&Computability::not_computable(Blocker::substance("blur", "filter")));
        assert_eq!(c.reasons(), ["strokes", "filter"]);
        assert_eq!(c.blockers()[1].key.to_string(), "substance:blur");
        assert!(!c.is_computable());
        let mut e = Computability::Exact;
        e.and(&Computability::Exact);
        assert!(e.is_exact());
        e.and(&Computability::replaced("dirt"));
        assert!(e.is_exact_or_replaced() && e.is_computable() && !e.is_exact());
        e.and(&Computability::approximated("guess"));
        assert!(!e.is_exact_or_replaced() && e.reasons() == ["guess"]);
    }
}
