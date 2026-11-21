//! Checks that the `mise` commands the docs show exist.
//!
//! Every `mise ...` command in a shell block or an inline code span under
//! `docs/` and in `README.md` is walked against mise's own usage spec: each
//! subcommand has to be one mise has, and each flag one that command accepts.
//! Values are not checked, so a placeholder such as `<tool>` is fine.
//!
//! A command that is wrong on purpose, or is not input to this mise (output,
//! a message mise prints, the `mise <task>` shorthand), is skipped with
//! `<!-- cli: skip -->`. On a line of its own, the marker skips the fenced
//! block or the heading that follows. Directly after an inline code span,
//! with no space between, it skips that span.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use usage::spec::arg::SpecDoubleDashChoices;
use usage::{SpecCommand, SpecFlag};

/// Fence languages whose blocks hold commands someone types.
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "shell",
    "console",
    "powershell",
    "nushell",
    "xonsh",
    "Dockerfile",
    "yaml",
];

/// Words that come before the command they run.
const PREFIXES: &[&str] = &[
    "$", "❯", "%", "sudo", "exec", "eval", "command", "time", "RUN", "run:", "-", "if", "!",
    "then", "do", "else", "&",
];

/// What mise prints after its own name in log lines, which console blocks show.
const LOG_LEVELS: &[&str] = &["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];

#[derive(Debug, Clone, PartialEq)]
struct Word {
    text: String,
    /// The word starts with a quote, so it is an argument and never a flag,
    /// a prefix or usage notation. `--flag='value'` is not quoted: only its
    /// value is.
    quoted: bool,
}

/// A `mise` command a page shows: the words after `mise`.
struct Shown {
    origin: String,
    words: Vec<Word>,
}

impl Shown {
    fn text(&self) -> String {
        let words: Vec<&str> = self.words.iter().map(|w| w.text.as_str()).collect();
        format!("mise {}", words.join(" "))
    }
}

#[test]
fn docs_show_commands_that_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // the corpus lives in the repository, not in the published crate
    if !root.join("docs").is_dir() {
        eprintln!("skipping: docs/ is only in a checkout of the repository");
        return;
    }
    let spec = super::usage::spec();
    let shown = corpus(root);
    // guards against the extraction silently finding nothing
    assert!(shown.len() > 4000, "found only {} commands", shown.len());

    let mut failures = String::new();
    for cmd in &shown {
        if let Err(problem) = check(&spec.cmd, &cmd.words) {
            writeln!(failures, "{}: `{}`: {problem}", cmd.origin, cmd.text()).unwrap();
        }
    }
    assert!(
        failures.is_empty(),
        "the docs show mise commands that mise does not accept ({} checked). Fix the \
         command. When it is not input to mise, put `<!-- cli: skip -->` on a line of its own \
         above the fenced block or heading, or directly after the inline code:\n{failures}",
        shown.len()
    );
}

fn corpus(root: &Path) -> Vec<Shown> {
    let mut shown = vec![];
    let mut files = vec![root.join("README.md")];
    let walk = walkdir::WalkDir::new(root.join("docs"))
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.file_name() != "node_modules" && e.file_name() != ".vitepress");
    for entry in walk {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "md") {
            files.push(entry.into_path());
        }
    }
    for file in files {
        let markdown = std::fs::read_to_string(&file).unwrap();
        let rel = file.strip_prefix(root).unwrap().display().to_string();
        for (line, words) in commands_in(&markdown) {
            shown.push(Shown {
                origin: format!("{rel}:{line}"),
                words,
            });
        }
    }
    shown
}

static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!--\s*cli:\s*skip\s*-->").unwrap());

/// The number of backticks a line opens with: three or more make a fence.
fn fence_len(line: &str) -> usize {
    let trimmed = line.trim_start();
    trimmed.len() - trimmed.trim_start_matches('`').len()
}

/// Whether the text right after a code span is the skip marker.
fn skips_span(after: &str) -> bool {
    SKIP.find(after).is_some_and(|marker| marker.start() == 0)
}

/// The `mise` commands in a Markdown document, with their 1-based line.
fn commands_in(markdown: &str) -> Vec<(usize, Vec<Word>)> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut found = vec![];
    // a marker on a line of its own skips the block or heading after it
    let mut skip_next = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let fence = fence_len(line);
        if fence < 3 {
            if !trimmed.is_empty() {
                let skipped_heading = skip_next && trimmed.starts_with('#');
                skip_next = SKIP.replace(line, "").trim().is_empty();
                for span in code_spans(line) {
                    if !skipped_heading {
                        found.extend(simple_commands(span).into_iter().map(|c| (i + 1, c)));
                    }
                }
            }
            i += 1;
            continue;
        }
        let lang = trimmed[fence..].split_whitespace().next().unwrap_or("");
        let checked = SHELLS.contains(&lang) && !skip_next;
        skip_next = false;
        i += 1;
        // a line ending in a backslash continues on the next one
        let mut logical = String::new();
        let mut start = i;
        while i < lines.len() {
            let content = lines[i].trim_start();
            i += 1;
            if content.len() >= fence && content.trim_end().chars().all(|c| c == '`') {
                break;
            }
            if logical.is_empty() {
                start = i;
            }
            match content.trim_end().strip_suffix('\\') {
                Some(head) => {
                    logical.push_str(head);
                    logical.push(' ');
                    continue;
                }
                None => logical.push_str(content),
            }
            if checked {
                found.extend(simple_commands(&logical).into_iter().map(|c| (start, c)));
            }
            logical.clear();
        }
    }
    found
}

/// The contents of the inline code spans on a line, without the skipped ones.
fn code_spans(line: &str) -> Vec<&str> {
    let mut spans = vec![];
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open..];
        let ticks = after.len() - after.trim_start_matches('`').len();
        let body = &after[ticks..];
        let closing = "`".repeat(ticks);
        let Some(close) = body.find(&closing) else {
            break;
        };
        rest = &body[close + ticks..];
        if !skips_span(rest) {
            spans.push(&body[..close]);
        }
    }
    spans
}

/// Splits a shell line into its simple commands and returns the ones that run
/// `mise`, as the words after `mise`. Redirections and comments end a command.
fn simple_commands(line: &str) -> Vec<Vec<Word>> {
    let mut split = Splitter::default();
    let mut quote: Option<char> = None;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        i += 1;
        // a command substitution starts a command of its own, also inside quotes
        if c == '$' && next == Some('(') && quote != Some('\'') {
            i += 1;
            quote = None;
            split.end_command();
            continue;
        }
        if let Some(q) = quote {
            match next {
                _ if c == q => quote = None,
                Some(escaped) if c == '\\' && q == '"' => {
                    split.cur.push(escaped);
                    i += 1;
                }
                _ => split.cur.push(c),
            }
            continue;
        }
        let bare = split.cur.is_empty() && !split.quoted;
        match c {
            '\'' | '"' => {
                quote = Some(c);
                split.quoted |= split.cur.is_empty();
            }
            c if c.is_whitespace() => split.end_word(),
            '#' if bare => break,
            // `2>&1` and `&>` are redirections, not the end of a command
            '&' if split.cur.is_empty() && (split.redirected || next == Some('>')) => {}
            ';' | '|' | '&' | '(' | ')' | '`' => split.end_command(),
            // a pipe inside a Markdown table is written `\|`
            '\\' if next == Some('|') => {}
            '<' if bare && is_placeholder_start(&chars[i..]) => split.cur.push(c),
            '>' if split.cur.contains('<') => split.cur.push(c),
            '<' | '>' if !split.quoted && split.cur.chars().all(|d| d.is_ascii_digit()) => {
                split.cur.clear();
                split.redirected = true;
            }
            _ => split.cur.push(c),
        }
    }
    split.end_command();

    split
        .commands
        .into_iter()
        .filter_map(|words| {
            let start = words.iter().position(|w| {
                w.quoted || !(PREFIXES.contains(&w.text.as_str()) || is_assignment(&w.text))
            })?;
            let (name, rest) = words[start..].split_first()?;
            (name.text == "mise" && !rest.is_empty()).then(|| rest.to_vec())
        })
        .collect()
}

/// The state of `simple_commands` between characters.
#[derive(Default)]
struct Splitter {
    commands: Vec<Vec<Word>>,
    words: Vec<Word>,
    cur: String,
    /// The word in `cur` starts with a quote, so it is a word even when empty.
    quoted: bool,
    /// A redirection was read: the rest of this command is not its arguments.
    redirected: bool,
}

impl Splitter {
    fn end_word(&mut self) {
        let text = std::mem::take(&mut self.cur);
        let quoted = std::mem::take(&mut self.quoted);
        if (quoted || !text.is_empty()) && !self.redirected {
            self.words.push(Word { text, quoted });
        }
    }

    fn end_command(&mut self) {
        self.end_word();
        self.commands.push(std::mem::take(&mut self.words));
        self.redirected = false;
    }
}

/// Whether the text after a `<` reads `name>`, so the `<` opens a placeholder
/// such as `<tool>` and is not a redirection.
fn is_placeholder_start(rest: &[char]) -> bool {
    let name: Vec<&char> = rest
        .iter()
        .take_while(|c| !c.is_whitespace() && **c != '>')
        .collect();
    !name.is_empty() && rest.get(name.len()) == Some(&'>')
}

fn is_assignment(word: &str) -> bool {
    static ASSIGNMENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=").unwrap());
    ASSIGNMENT.is_match(word)
}

/// A word that stands for something the reader fills in.
fn is_placeholder(word: &str) -> bool {
    let upper = |c: char| c.is_ascii_uppercase() || c == '_';
    // `COMMAND` and `TOOL@VERSION` in a usage line
    (word.len() > 1 && word.split('@').all(|part| part.chars().all(upper)))
        || word.starts_with('<')
        || word.starts_with('{')
        || word.contains('…')
        || word.contains("...")
}

/// Walks the words after `mise` through the usage spec.
fn check(root: &SpecCommand, words: &[Word]) -> Result<(), String> {
    // the commands entered so far; a global flag of any of them is accepted
    let mut path = vec![root];
    let mut positionals = 0;
    let mut i = 0;
    if words
        .first()
        .is_some_and(|w| LOG_LEVELS.contains(&w.text.as_str()) || w.text == "=")
    {
        return Ok(());
    }
    while i < words.len() {
        let word = &words[i];
        let text = word.text.as_str();
        let cmd = *path.last().unwrap();
        i += 1;
        // `[--json]`, `[FLAGS]` and `<--brew>` are usage notation: the line
        // describes the command and is not something to type
        if !word.quoted && (text.starts_with('[') || text.starts_with("<-")) {
            return Ok(());
        }
        if word.quoted || is_placeholder(text) {
            if positional(cmd, &mut positionals, text)? {
                return Ok(());
            }
            continue;
        }
        if text == "--" {
            return Ok(());
        }
        if let Some(long) = text.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, _)) => (name, true),
                None => (long, false),
            };
            match find_flag(&path, |f| {
                f.long.iter().chain(&f.hidden_aliases).any(|l| l == name)
                    || f.negate.as_deref() == Some(&format!("--{name}"))
            }) {
                Some(flag) => {
                    if !value && takes_value(flag, cmd, words.get(i)) {
                        i += 1;
                    }
                }
                // every command prints its help, also the ones that declare no
                // help flag because they answer it themselves
                None if name == "help" => return Ok(()),
                None if forwards_unknown_flags(cmd) => {
                    if positional(cmd, &mut positionals, text)? {
                        return Ok(());
                    }
                }
                None => return Err(format!("{} has no flag --{name}", name_of(cmd))),
            }
            continue;
        }
        let is_short = text.len() > 1
            && text.starts_with('-')
            && !text[1..].starts_with(|c: char| c.is_ascii_digit());
        if is_short {
            let cluster: Vec<char> = text[1..].chars().collect();
            for (n, c) in cluster.iter().enumerate() {
                let found = find_flag(&path, |f| {
                    f.short.contains(c) || f.hidden_short_aliases.contains(c)
                });
                match found {
                    Some(flag) => {
                        let last = n + 1 == cluster.len();
                        if flag.arg.is_some() {
                            // the rest of the cluster, or the next word, is the value
                            if last && takes_value(flag, cmd, words.get(i)) {
                                i += 1;
                            }
                            break;
                        }
                    }
                    None if *c == 'h' => return Ok(()),
                    None if forwards_unknown_flags(cmd) => {
                        if positional(cmd, &mut positionals, text)? {
                            return Ok(());
                        }
                        break;
                    }
                    None => return Err(format!("{} has no flag -{c}", name_of(cmd))),
                }
            }
            continue;
        }
        if positionals == 0
            && let Some(sub) = subcommand(cmd, text)
        {
            path.push(sub);
            continue;
        }
        if cmd.full_cmd.is_empty() && positionals == 0 && text == "help" {
            // `mise help <command>` prints the help of the command that follows
            continue;
        }
        if cmd.full_cmd.is_empty() && (text.starts_with("//") || text.starts_with(':')) {
            // a monorepo task path, which no command name can be
            return Ok(());
        }
        if cmd.full_cmd.is_empty() {
            // the usage spec lets an unknown word be a task (`mise <task>`), which
            // would let every misspelled command through
            return Err(format!(
                "mise has no command `{text}`; write `mise run {text}` for a task"
            ));
        }
        if positional(cmd, &mut positionals, text)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Takes one positional word. `Ok(true)` when everything after it belongs to
/// the program or task mise runs, which this check knows nothing about.
fn positional(cmd: &SpecCommand, positionals: &mut usize, text: &str) -> Result<bool, String> {
    let index = *positionals;
    *positionals += 1;
    if !cmd.mounts.is_empty() {
        // the word is a task name, and the rest are the task's own arguments
        return match text.strip_prefix('-') {
            Some(_) if index == 0 => Err(format!("{} has no flag {text}", name_of(cmd))),
            _ => Ok(true),
        };
    }
    if cmd.clause.as_ref().is_some_and(|c| !c.args.is_empty()) {
        return Ok(false);
    }
    let arg = match cmd.args.get(index) {
        Some(arg) => arg,
        None => match cmd.args.last() {
            Some(last) if last.var => last,
            _ if cmd.args.is_empty() && !cmd.subcommands.is_empty() => {
                // `mise bootstrap <part> apply`: which subcommand is not known
                return if is_placeholder(text) {
                    Ok(true)
                } else {
                    Err(format!("{} has no subcommand `{text}`", name_of(cmd)))
                };
            }
            _ => return Err(format!("{} takes no argument `{text}`", name_of(cmd))),
        },
    };
    Ok(arg.double_dash == SpecDoubleDashChoices::Automatic)
}

fn name_of(cmd: &SpecCommand) -> String {
    if cmd.full_cmd.is_empty() {
        "mise".to_string()
    } else {
        format!("`mise {}`", cmd.full_cmd.join(" "))
    }
}

fn subcommand<'a>(cmd: &'a SpecCommand, name: &str) -> Option<&'a SpecCommand> {
    cmd.subcommands.values().find(|sub| {
        sub.name == name
            || sub
                .aliases
                .iter()
                .chain(&sub.hidden_aliases)
                .any(|alias| alias == name)
    })
}

/// A flag of the innermost command, or a global flag of one it sits under.
fn find_flag<'a>(
    path: &[&'a SpecCommand],
    matches: impl Fn(&SpecFlag) -> bool,
) -> Option<&'a SpecFlag> {
    let (cmd, parents) = path.split_last()?;
    // a clause is a group of flags and arguments that repeats, as in
    // `mise use --tool-option k=v node@24 python@3.14`
    let clause = cmd.clause.iter().flat_map(|clause| &clause.flags);
    cmd.flags
        .iter()
        .chain(clause)
        .find(|f| matches(f))
        .or_else(|| {
            parents
                .iter()
                .rev()
                .flat_map(|parent| &parent.flags)
                .find(|f| f.global && matches(f))
        })
}

/// Whether the word after a flag is that flag's value.
fn takes_value(flag: &SpecFlag, cmd: &SpecCommand, next: Option<&Word>) -> bool {
    let Some(arg) = &flag.arg else {
        return false;
    };
    // an optional value is only taken when the next word can be one
    arg.required
        || next.is_some_and(|next| {
            next.quoted || !(next.text.starts_with('-') || subcommand(cmd, &next.text).is_some())
        })
}

/// Whether the command hands a flag it does not know to its arguments.
fn forwards_unknown_flags(cmd: &SpecCommand) -> bool {
    cmd.unknown_flags == Some(usage::spec::unknown_flags::UnknownFlags::Value)
}

mod tests {
    use super::*;

    fn shown(line: &str) -> Vec<String> {
        simple_commands(line)
            .into_iter()
            .map(|words| {
                let words: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
                words.join(" ")
            })
            .collect()
    }

    /// Takes the spec, which is slow to build, so a test builds it once.
    fn verdict(spec: &usage::Spec, line: &str) -> Result<(), String> {
        let commands = simple_commands(line);
        assert_eq!(commands.len(), 1, "{line}");
        check(&spec.cmd, &commands[0])
    }

    #[test]
    fn finds_mise_where_a_command_starts() {
        assert_eq!(shown("$ mise use node@24"), ["use node@24"]);
        assert_eq!(
            shown("cd app && MISE_ENV=ci mise install | tee log"),
            ["install"]
        );
        assert_eq!(shown(r#"eval "$(mise activate bash)""#), ["activate bash"]);
        assert_eq!(
            shown("mise ls --json > tools.json # all tools"),
            ["ls --json"]
        );
        assert_eq!(shown("mise use <tool>@<version>"), ["use <tool>@<version>"]);
        assert_eq!(shown("mise run build 2>&1"), ["run build"]);
        assert_eq!(shown(r"mise activate fish \| source"), ["activate fish"]);
        assert_eq!(shown("echo mise install"), Vec::<String>::new());
        assert_eq!(shown("mise"), Vec::<String>::new());
    }

    #[test]
    fn reads_fenced_blocks_and_inline_code() {
        let markdown = "Run `mise install` first.\n\n```sh\nmise use \\\n  node@24\n```\n\n\
                        ```toml\nrun = \"mise bogus\"\n```\n\n<!-- cli: skip -->\n```sh\nmise bogus\n```\n\
                        A message, `mise bogus`<!-- cli: skip -->, then `mise ls`.\n\n\
                        <!-- cli: skip -->\n\n## `mise bogus` is printed\n\nThen `mise doctor`.\n";
        let found: Vec<(usize, String)> = commands_in(markdown)
            .into_iter()
            .map(|(line, words)| (line, words[0].text.clone()))
            .collect();
        assert_eq!(
            found,
            [
                (1, "install".to_string()),
                (4, "use".to_string()),
                (16, "ls".to_string()),
                (22, "doctor".to_string())
            ]
        );
    }

    #[test]
    fn accepts_what_mise_accepts() {
        let spec = crate::cli::usage::spec();
        for line in [
            "mise use -g node@24",
            "mise install --jobs 4 node",
            "mise -C app run --jobs=2 build --release",
            "mise x node@24 -- node --bogus",
            "mise settings set <key> <value>",
            "mise ls -J",
            "mise watch build -- --exts rs",
            "mise run --help",
            "mise use --tool-option k=v node@24 python@3.14",
            "mise bootstrap <part> apply",
            "mise daemons ls [--json]",
            "mise //projects/frontend:build",
            "mise COMMAND --help",
            "mise help lsp",
            "mise sync ruby <--brew>",
            "mise exec --allow-env='MYAPP_*' -- node app.js",
        ] {
            assert_eq!(verdict(&spec, line), Ok(()), "{line}");
        }
    }

    #[test]
    fn rejects_what_mise_rejects() {
        let spec = crate::cli::usage::spec();
        for (line, problem) in [
            ("mise instal node", "mise has no command `instal`"),
            ("mise install --bogus", "`mise install` has no flag --bogus"),
            (
                "mise exec --bogus='MYAPP_*' -- node app.js",
                "`mise exec` has no flag --bogus",
            ),
            ("mise ls -Z", "`mise ls` has no flag -Z"),
            (
                "mise settings bogus x y z",
                "`mise settings` takes no argument",
            ),
            ("mise run --bogus build", "`mise run` has no flag --bogus"),
            ("mise cache bogus", "`mise cache` has no subcommand `bogus`"),
            ("mise help bogus", "mise has no command `bogus`"),
        ] {
            let got = verdict(&spec, line).unwrap_err();
            assert!(got.contains(problem), "{line}: {got}");
        }
    }
}
