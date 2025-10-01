pub struct SelfUpdate {}

impl SelfUpdate {
    pub fn is_available() -> bool {
        false
    }
}

pub fn upgrade_instructions_text() -> Option<String> {
    None
}

pub fn append_self_update_instructions(message: String) -> String {
    message
}
