//! Natural (numeric-aware) string ordering, so `icon-2.png` sorts before
//! `icon-10.png` instead of after it — the plain lexicographic order git and
//! a naive `sort` produce.
//!
//! Shared by the Changes pane (`git::branch_changes`) and the Files tab
//! (`files::list_dir`) so both list numbered files in numeric order.

use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

/// Compare two strings with runs of ASCII digits compared as numbers:
/// `"icon-2"` < `"icon-10"`. Every other character compares by `char` order
/// (case-sensitive, matching git's own path ordering), so this only changes
/// the relative position of digit runs.
pub fn compare(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) if ac.is_ascii_digit() && bc.is_ascii_digit() => {
                let ord = compare_numeric(&take_digits(&mut a_chars), &take_digits(&mut b_chars));
                if ord != Ordering::Equal {
                    return ord;
                }
                // Equal numeric value — keep comparing the rest.
            }
            (Some(ac), Some(bc)) => {
                let ord = ac.cmp(&bc);
                if ord != Ordering::Equal {
                    return ord;
                }
                a_chars.next();
                b_chars.next();
            }
        }
    }
}

/// Consume and return the run of ASCII digits at the head of `chars` (which
/// is non-empty and starts with one). Comparing runs as trimmed strings
/// avoids parsing and any integer-overflow ceiling.
fn take_digits(chars: &mut Peekable<Chars>) -> String {
    let mut run = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            run.push(c);
            chars.next();
        } else {
            break;
        }
    }
    run
}

/// Order two numeric runs by value (leading zeros ignored), falling back to
/// the raw digits so equal values with different spellings (`"2"` vs `"02"`)
/// still order deterministically — a comparator that returned `Equal` for
/// distinct strings wouldn't be a total order.
fn compare_numeric(a: &str, b: &str) -> Ordering {
    let (a_trimmed, b_trimmed) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a_trimmed
        .len()
        .cmp(&b_trimmed.len())
        .then_with(|| a_trimmed.cmp(b_trimmed))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut items: Vec<&str>) -> Vec<&str> {
        items.sort_by(|a, b| compare(a, b));
        items
    }

    #[test]
    fn orders_digit_runs_numerically() {
        assert_eq!(
            sorted(vec![
                "assets/icon-128.png",
                "assets/icon-16.png",
                "assets/icon-24.png",
                "assets/icon-256.png",
                "assets/icon-32.png",
                "assets/icon-48.png",
                "assets/icon-512.png",
                "assets/icon-64.png",
            ]),
            vec![
                "assets/icon-16.png",
                "assets/icon-24.png",
                "assets/icon-32.png",
                "assets/icon-48.png",
                "assets/icon-64.png",
                "assets/icon-128.png",
                "assets/icon-256.png",
                "assets/icon-512.png",
            ]
        );
    }

    #[test]
    fn shorter_digit_run_sorts_first_within_the_same_prefix() {
        assert_eq!(compare("icon-2.png", "icon-10.png"), Ordering::Less);
        assert_eq!(compare("icon-10.png", "icon-2.png"), Ordering::Greater);
    }

    #[test]
    fn equal_orders_are_a_total_order() {
        assert_eq!(compare("a", "a"), Ordering::Equal);
        assert_eq!(compare("a", "ab"), Ordering::Less);
        // Equal numeric value, different spelling — still ordered, not Equal.
        assert_ne!(compare("a2", "a02"), Ordering::Equal);
        assert_eq!(compare("a0", "a0"), Ordering::Equal);
    }

    #[test]
    fn plain_alpha_and_case_order_are_preserved() {
        assert_eq!(
            sorted(vec!["b.txt", "a.txt", "c.txt"]),
            vec!["a.txt", "b.txt", "c.txt"]
        );
        // Case-sensitive, like git's path ordering (uppercase before lowercase).
        assert_eq!(compare("README.md", "assets/icon.svg"), Ordering::Less);
    }
}
