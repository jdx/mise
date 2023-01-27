use std::io::stdin;

pub fn prompt() -> String {
    let mut input = String::new();
    stdin().read_line(&mut input).expect("error reading stdin");

    input.trim().to_string()
}
