#[must_use]
#[derive(Debug)]
pub struct HandleGuard();

/// ensures cursor is displayed on ctrl-c
pub fn handle_ctrlc() -> eyre::Result<Option<HandleGuard>> {
    Ok(Some(HandleGuard()))
}
