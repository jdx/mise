use console::style;
use std::io::stdin;

pub fn prompt() -> String {
    let mut input = String::new();
    stdin().read_line(&mut input).expect("error reading stdin");

    input.trim().to_string()
}

pub fn prompt_for_install(thing: &str) -> bool {
    match console::user_attended_stderr() {
        true => {
            let stderr = console::Term::stderr();
            eprint!(
                "{} Would you like to install {}? [Y/n] ",
                style("rtx").dim().for_stderr(),
                thing,
            );
            let yn = matches!(prompt().to_lowercase().as_str(), "" | "y" | "yes");
            let _ = stderr.move_cursor_up(1);
            let _ = stderr.clear_to_end_of_screen();
            yn
        }
        false => false,
    }
}
