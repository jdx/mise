//! Release-version gate, deliberately independent of the wall clock.

pub(crate) fn check_release(year: u32, month: u32) {
    assert!(
        (year, month) < (2026, 12),
        "Review lockfile_mode=generate trial feedback: promote generate to the default or explicitly postpone this December 2026 decision gate"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_before_december_pass() {
        check_release(2026, 9);
        check_release(2026, 11);
    }

    #[test]
    #[should_panic(expected = "Review lockfile_mode=generate trial feedback")]
    fn first_december_release_requires_decision() {
        check_release(2026, 12);
    }

    #[test]
    #[should_panic(expected = "explicitly postpone")]
    fn later_releases_cannot_bypass_decision() {
        check_release(2027, 1);
    }
}
