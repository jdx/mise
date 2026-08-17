/// Normalize a requested concurrency limit to the minimum usable value.
///
/// A Tokio semaphore with zero permits can never start its first job, so all
/// user-facing job counts treat zero as serial execution.
pub fn normalize(jobs: usize) -> usize {
    jobs.max(1)
}

/// Resolve a command-specific override before applying the minimum.
pub fn resolve(configured: usize, override_: Option<usize>) -> usize {
    normalize(override_.unwrap_or(configured))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_zero_to_one() {
        assert_eq!(normalize(0), 1);
    }

    #[test]
    fn preserves_positive_values() {
        assert_eq!(normalize(1), 1);
        assert_eq!(normalize(8), 8);
    }

    #[test]
    fn override_takes_precedence_before_normalization() {
        assert_eq!(resolve(8, Some(2)), 2);
        assert_eq!(resolve(8, Some(0)), 1);
        assert_eq!(resolve(0, None), 1);
    }
}
