use rand::RngExt;
use rand::distr::Alphanumeric;

pub(crate) fn random_string(length: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect::<String>()
}
