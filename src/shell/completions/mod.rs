use clap::Command;

mod zsh_complete;

pub fn zsh_complete(cmd: &Command) -> eyre::Result<String> {
    let output = zsh_complete::render(cmd);
    let result = cmd!("shfmt", "-s").stdin_bytes(output).read()?;
    Ok(result)
}
