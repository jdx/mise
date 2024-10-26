pub fn init() {}

// pub fn add_handler(_func: impl Fn() + Send + Sync + 'static) {}

pub fn exit_on_ctrl_c(_do_exit: bool) {}

/// ensures cursor is displayed on ctrl-c
pub fn show_cursor_after_ctrl_c() {}
