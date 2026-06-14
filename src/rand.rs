use rand::RngExt;
use rand::distr::Alphanumeric;

/// Generates a random string of alphanumeric characters with the given length.
///
/// # Arguments
///
/// * `length` - The desired length of the random string.
///
/// # Returns
///
/// A `String` containing `length` random alphanumeric characters.
pub fn random_string(length: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect::<String>()
}
