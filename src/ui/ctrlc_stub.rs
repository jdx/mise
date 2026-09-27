pub async fn exit_signal() -> i32 {
    std::future::pending().await
}

// pub fn add_handler(_func: impl Fn() + Send + Sync + 'static) {}

pub fn exit_on_ctrl_c(_do_exit: bool) {}

pub fn is_cancelled() -> bool {
    false
}

/// ensures cursor is displayed on ctrl-c
pub fn show_cursor_after_ctrl_c() {}
