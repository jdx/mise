pub use mise_util::semver::*;

use std::cmp::Ordering;

pub(crate) fn semver_precedence_cmp(current: &str, candidate: &str) -> Option<Ordering> {
    let parse = |v: &str| ::semver::Version::parse(v.strip_prefix(['v', 'V']).unwrap_or(v)).ok();
    Some(parse(current)?.cmp_precedence(&parse(candidate)?))
}
