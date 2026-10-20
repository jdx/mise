//! The LSP session: requests in, responses and diagnostics out.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::ops::Range;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::{Value, json};

use crate::complete::{self, Kind};
use crate::diagnostics::{self, Diagnostic, Severity};
use crate::rpc::{read_message, write_message};
use crate::schema::Schema;
use crate::text::LineIndex;
use crate::{Formatter, Options, hover, includes, keypath};

const METHOD_NOT_FOUND: i64 = -32601;

/// What an editor can switch off in `initializationOptions`, for use next to another TOML
/// server that already offers it: `{"formatting": false, "hover": false, "completion": false}`.
struct Features {
    formatting: bool,
    hover: bool,
    completion: bool,
}

impl Features {
    fn from_params(params: &Value) -> Self {
        let option = |name: &str| {
            params["initializationOptions"][name]
                .as_bool()
                .unwrap_or(true)
        };
        Self {
            formatting: option("formatting"),
            hover: option("hover"),
            completion: option("completion"),
        }
    }
}

struct Document {
    text: String,
    /// A file `[task_config] includes` names: tasks, not a config.
    include: bool,
    /// The last version that parsed, with its schema diagnostics, so that typing a half-written
    /// line does not take the other diagnostics away.
    last_good: Option<Good>,
}

struct Good {
    text: String,
    diagnostics: Vec<Diagnostic>,
}

pub(crate) struct Server {
    schema: Schema,
    validator: OnceLock<Option<Validator>>,
    format: Option<Formatter>,
    version: String,
    features: Features,
    documents: HashMap<String, Document>,
}

impl Server {
    pub(crate) fn new(options: Options) -> Self {
        Self {
            schema: Schema::new(options.schema),
            validator: OnceLock::new(),
            format: options.format,
            version: options.version,
            features: Features::from_params(&Value::Null),
            documents: HashMap::new(),
        }
    }

    pub(crate) fn run(mut self, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        while let Some(message) = read_message(&mut input)? {
            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue; // a response to a request we never send
            };
            let params = message.get("params").unwrap_or(&Value::Null);
            match message.get("id") {
                Some(id) => {
                    let reply = match self.request(method, params) {
                        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                        Err((code, message)) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {"code": code, "message": message},
                        }),
                    };
                    write_message(&mut output, &reply)?;
                }
                None if method == "exit" => return Ok(()),
                None => self.notification(method, params, &mut output)?,
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                self.features = Features::from_params(params);
                Ok(self.initialize())
            }
            "shutdown" => Ok(Value::Null),
            "textDocument/completion" if self.features.completion => Ok(self.completion(params)),
            "textDocument/hover" if self.features.hover => Ok(self.hover(params)),
            "textDocument/formatting" if self.format.is_some() && self.features.formatting => {
                Ok(self.formatting(params))
            }
            "textDocument/codeAction" => Ok(self.code_actions(params)),
            _ => Err((METHOD_NOT_FOUND, format!("method not supported: {method}"))),
        }
    }

    fn notification(
        &mut self,
        method: &str,
        params: &Value,
        output: &mut impl Write,
    ) -> io::Result<()> {
        let document = &params["textDocument"];
        let Some(uri) = document["uri"].as_str() else {
            return Ok(());
        };
        match method {
            "textDocument/didOpen" => {
                let text = document["text"].as_str().unwrap_or_default().to_string();
                let include = includes::uri_path(uri)
                    .is_some_and(|path| is_mise_config(uri) && includes::is_task_include(&path));
                self.documents.insert(
                    uri.to_string(),
                    Document {
                        text,
                        include,
                        last_good: None,
                    },
                );
            }
            "textDocument/didChange" => {
                // The server asks for full documents, so the last change is the whole text.
                let Some(text) = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str())
                else {
                    return Ok(());
                };
                if let Some(document) = self.documents.get_mut(uri) {
                    document.text = text.to_string();
                }
            }
            "textDocument/didClose" => {
                self.documents.remove(uri);
                return publish(output, uri, json!([]));
            }
            _ => return Ok(()),
        }
        let diagnostics = self.diagnostics(uri);
        publish(output, uri, diagnostics)
    }

    fn initialize(&self) -> Value {
        let mut capabilities = json!({
            "textDocumentSync": {"openClose": true, "change": 1},
            "codeActionProvider": {"codeActionKinds": ["quickfix"]},
        });
        if self.features.completion {
            capabilities["completionProvider"] =
                json!({"triggerCharacters": [".", "=", "[", "\"", "{", ","]});
        }
        if self.features.hover {
            capabilities["hoverProvider"] = json!(true);
        }
        if self.format.is_some() && self.features.formatting {
            capabilities["documentFormattingProvider"] = json!(true);
        }
        json!({
            "capabilities": capabilities,
            "serverInfo": {"name": "mise", "version": self.version},
        })
    }

    /// The open mise config at `uri`, or `None` for any other file: editors attach a TOML server
    /// to every TOML file, and `Cargo.toml` is not ours to judge. A task include is not a config
    /// either.
    fn config(&self, uri: &str) -> Option<&Document> {
        if !is_mise_config(uri) {
            return None;
        }
        self.documents.get(uri).filter(|document| !document.include)
    }

    fn diagnostics(&mut self, uri: &str) -> Value {
        let Some(document) = self.config(uri) else {
            return json!([]);
        };
        let validator = self
            .validator
            .get_or_init(|| jsonschema::draft202012::new(self.schema.root()).ok())
            .as_ref();
        let text = document.text.clone();
        let report = diagnostics::diagnose(&text, &self.schema, validator);
        let document = self.documents.get_mut(uri).expect("checked above");
        let mut diagnostics = report.diagnostics;
        if report.parsed {
            document.last_good = Some(Good {
                text: text.clone(),
                diagnostics: diagnostics.clone(),
            });
        } else if let Some(good) = &document.last_good {
            // Keep what the last good version said about the parts of the file the edit did not
            // touch, next to the syntax error.
            let edit = Edit::between(&good.text, &text);
            diagnostics.extend(good.diagnostics.iter().filter_map(|d| {
                let range = edit.map_range(&d.range)?;
                Some(Diagnostic { range, ..d.clone() })
            }));
        }
        let index = LineIndex::new(&text);
        json!(
            diagnostics
                .iter()
                .map(|d| diagnostic_json(&index, d))
                .collect::<Vec<_>>()
        )
    }

    fn completion(&self, params: &Value) -> Value {
        let Some((document, offset)) = self.cursor(params) else {
            return Value::Null;
        };
        let text = document.text.as_str();
        let index = LineIndex::new(text);
        let items: Vec<Value> = complete::complete(text, offset, &self.schema)
            .into_iter()
            .map(|item| {
                let mut value = json!({
                    "label": item.label,
                    "kind": match item.kind {
                        Kind::Key => 10,
                        Kind::Value => 12,
                    },
                    "filterText": item.insert,
                    "textEdit": {"range": range(&index, &item.replace), "newText": item.insert},
                });
                if let Some(detail) = item.detail {
                    value["detail"] = json!(detail);
                }
                if let Some(documentation) = item.documentation {
                    value["documentation"] = json!({"kind": "markdown", "value": documentation});
                }
                value
            })
            .collect();
        json!({"isIncomplete": false, "items": items})
    }

    fn hover(&self, params: &Value) -> Value {
        let Some((document, offset)) = self.cursor(params) else {
            return Value::Null;
        };
        let text = document.text.as_str();
        if let Some(h) = hover::hover(text, offset, &self.schema) {
            return json!({
                "contents": {"kind": "markdown", "value": h.markdown},
                "range": range(&LineIndex::new(text), &h.range),
            });
        }
        // The text does not parse right now: answer from the last version that did, for a
        // position the edit did not touch.
        let Some(good) = &document.last_good else {
            return Value::Null;
        };
        let edit = Edit::between(&good.text, text);
        let hovered = edit
            .map_offset_back(offset)
            .and_then(|old| hover::hover(&good.text, old, &self.schema));
        match hovered {
            Some(h) => json!({"contents": {"kind": "markdown", "value": h.markdown}}),
            None => Value::Null,
        }
    }

    fn formatting(&self, params: &Value) -> Value {
        let (Some(format), Some(document)) = (
            &self.format,
            params["textDocument"]["uri"]
                .as_str()
                .and_then(|uri| self.config(uri)),
        ) else {
            return Value::Null;
        };
        let text = document.text.as_str();
        match format(text) {
            Ok(formatted) if formatted != text => {
                let index = LineIndex::new(text);
                json!([{"range": range(&index, &(0..text.len())), "newText": formatted}])
            }
            // Unparseable text has no formatting; the diagnostics already say why.
            _ => json!([]),
        }
    }

    /// Quick fixes: replace an unknown key with the one the diagnostic suggests.
    fn code_actions(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let document = self.config(uri);
        let actions: Vec<Value> = params["context"]["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["code"] == "unknown-key")
            .filter_map(|d| {
                let suggestion = d["data"]["suggestion"].as_str()?;
                // The diagnostic may be stale: only fix the key it was about.
                let key = d["data"]["key"].as_str()?;
                let text = &document?.text;
                let index = LineIndex::new(text);
                let at = |p: &Value| {
                    index.offset(
                        p["line"].as_u64().unwrap_or_default() as usize,
                        p["character"].as_u64().unwrap_or_default() as usize,
                    )
                };
                let found = text.get(at(&d["range"]["start"])..at(&d["range"]["end"]))?;
                // The key as TOML reads it, so an escaped spelling still matches.
                let unchanged = toml_edit::Key::parse(found)
                    .is_ok_and(|keys| keys.len() == 1 && keys[0].get() == key);
                if !unchanged {
                    return None;
                }
                Some(json!({
                    "title": format!("Replace with `{suggestion}`"),
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "isPreferred": true,
                    "edit": {"changes": {uri: [{
                        "range": d["range"],
                        "newText": keypath::quote(suggestion),
                    }]}},
                }))
            })
            .collect();
        json!(actions)
    }

    /// The document and byte offset a position request is about.
    fn cursor(&self, params: &Value) -> Option<(&Document, usize)> {
        let document = self.config(params["textDocument"]["uri"].as_str()?)?;
        let line = params["position"]["line"].as_u64()? as usize;
        let character = params["position"]["character"].as_u64()? as usize;
        let offset = LineIndex::new(&document.text).offset(line, character);
        Some((document, offset))
    }
}

/// What changed between two versions of a text: everything outside one replaced region is the
/// same, so a position there can be carried from one version to the other.
struct Edit {
    old_len: usize,
    new_len: usize,
    prefix: usize,
    suffix: usize,
}

impl Edit {
    fn between(old: &str, new: &str) -> Self {
        let mut prefix = old
            .bytes()
            .zip(new.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let limit = old.len().min(new.len()) - prefix;
        let mut suffix = old
            .bytes()
            .rev()
            .zip(new.bytes().rev())
            .take_while(|(a, b)| a == b)
            .count()
            .min(limit);
        while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix)
        {
            suffix -= 1;
        }
        Self {
            old_len: old.len(),
            new_len: new.len(),
            prefix,
            suffix,
        }
    }

    /// A range of the old text in the new one, unless the edit touched it.
    fn map_range(&self, range: &Range<usize>) -> Option<Range<usize>> {
        if range.end <= self.prefix {
            return Some(range.clone());
        }
        if range.start >= self.old_len - self.suffix {
            let shift = |o: usize| o + self.new_len - self.old_len;
            return Some(shift(range.start)..shift(range.end));
        }
        None
    }

    /// An offset of the new text in the old one, unless it is inside the edit.
    fn map_offset_back(&self, offset: usize) -> Option<usize> {
        if offset <= self.prefix {
            return Some(offset);
        }
        if offset >= self.new_len - self.suffix {
            return Some(offset + self.old_len - self.new_len);
        }
        None
    }
}

fn diagnostic_json(index: &LineIndex, d: &Diagnostic) -> Value {
    let mut diagnostic = json!({
        "range": range(index, &d.range),
        "severity": match d.severity {
            Severity::Error => 1,
            Severity::Warning => 2,
        },
        "source": "mise",
        "code": d.code,
        "message": d.message,
    });
    if d.deprecated {
        diagnostic["tags"] = json!([2]);
    }
    if let Some(suggestion) = &d.suggestion {
        diagnostic["data"] = json!({"suggestion": suggestion, "key": d.key});
    }
    diagnostic
}

fn publish(output: &mut impl Write, uri: &str, diagnostics: Value) -> io::Result<()> {
    write_message(
        output,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "diagnostics": diagnostics},
        }),
    )
}

fn range(index: &LineIndex, span: &std::ops::Range<usize>) -> Value {
    let (start_line, start) = index.position(span.start);
    let (end_line, end) = index.position(span.end);
    json!({
        "start": {"line": start_line, "character": start},
        "end": {"line": end_line, "character": end},
    })
}

/// Whether the file at `uri` is one mise loads as a config; see [`includes::is_config_file`].
fn is_mise_config(uri: &str) -> bool {
    uri.strip_prefix("file://")
        .is_some_and(|path| includes::is_config_file(&includes::percent_decode(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::sample;

    /// Runs a whole session and returns every message the server wrote.
    fn session(messages: &[Value]) -> Vec<Value> {
        let mut input = vec![];
        for message in messages {
            write_message(&mut input, message).unwrap();
        }
        let mut output = vec![];
        let options = Options {
            schema: sample().root().clone(),
            format: Some(Box::new(|text| Ok(format!("# formatted\n{text}")))),
            version: "1".to_string(),
        };
        Server::new(options)
            .run(input.as_slice(), &mut output)
            .unwrap();
        let mut reader = output.as_slice();
        std::iter::from_fn(|| read_message(&mut reader).unwrap()).collect()
    }

    fn open(uri: &str, text: &str) -> Value {
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "text": text}}})
    }

    #[test]
    fn a_session_publishes_diagnostics_and_answers_requests() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            open(uri, "[settings]\ncolr = true\n"),
            json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/completion",
                "params": {"textDocument": {"uri": uri}, "position": {"line": 1, "character": 2}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/formatting",
                "params": {"textDocument": {"uri": uri}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "nope"}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
            // after exit: never read
            json!({"jsonrpc": "2.0", "id": 5, "method": "shutdown"}),
        ]);
        assert_eq!(replies.len(), 5);
        assert_eq!(replies[0]["result"]["capabilities"]["hoverProvider"], true);
        assert_eq!(
            replies[1]["params"]["diagnostics"][0]["message"],
            "unknown key `colr`; did you mean `color`?"
        );
        assert_eq!(replies[2]["result"]["items"][0]["label"], "color");
        assert_eq!(
            replies[3]["result"][0]["newText"],
            "# formatted\n[settings]\ncolr = true\n"
        );
        assert_eq!(replies[4]["error"]["code"], METHOD_NOT_FOUND);
    }

    fn change(uri: &str, text: &str) -> Value {
        json!({"jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {"textDocument": {"uri": uri}, "contentChanges": [{"text": text}]}})
    }

    #[test]
    fn diagnostics_survive_a_half_typed_line() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "[settings]\ncolr = true\n"),
            change(uri, "[settings]\ncolr = true\nco"),
        ]);
        let messages = |i: usize| -> Vec<String> {
            replies[i]["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["code"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(messages(0), ["unknown-key"]);
        assert_eq!(messages(1), ["syntax", "unknown-key"]);
    }

    #[test]
    fn an_edit_over_a_diagnostic_drops_it() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "[settings]\ncolr = true\n"),
            change(uri, "[settings]\ncol = \n"),
        ]);
        let codes: Vec<_> = replies[1]["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, ["syntax"]);
    }

    #[test]
    fn features_can_be_switched_off() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "initializationOptions": {"hover": false, "completion": false, "formatting": false}
            }}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover",
                "params": {"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}}}),
        ]);
        let capabilities = &replies[0]["result"]["capabilities"];
        assert!(capabilities.get("hoverProvider").is_none());
        assert!(capabilities.get("completionProvider").is_none());
        assert!(capabilities.get("documentFormattingProvider").is_none());
        assert!(capabilities.get("codeActionProvider").is_some());
        assert_eq!(replies[1]["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn unknown_keys_get_a_quick_fix() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "[settings]\ncolr = true\n"),
            json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}},
                "context": {"diagnostics": [replies_diagnostic()]},
            }}),
        ]);
        let action = &replies[1]["result"][0];
        assert_eq!(action["title"], "Replace with `color`");
        assert_eq!(action["edit"]["changes"][uri][0]["newText"], "color");
    }

    #[test]
    fn an_escaped_key_still_gets_its_quick_fix() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "[settings]\n\"col\\u0072\" = true\n"),
            json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 11}},
                "context": {"diagnostics": [{"code": "unknown-key",
                    "data": {"suggestion": "color", "key": "colr"},
                    "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 11}}}]},
            }}),
        ]);
        assert_eq!(replies[1]["result"][0]["title"], "Replace with `color`");
    }

    #[test]
    fn a_stale_diagnostic_gets_no_quick_fix() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "[settings]\ncolo = true\n"),
            json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/codeAction", "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}},
                "context": {"diagnostics": [replies_diagnostic()]},
            }}),
        ]);
        assert_eq!(replies[1]["result"], json!([]));
    }

    fn replies_diagnostic() -> Value {
        json!({"code": "unknown-key", "data": {"suggestion": "color", "key": "colr"},
            "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}})
    }

    #[test]
    fn task_include_files_are_not_configs() {
        let dir = std::env::temp_dir().join(format!("mise-lsp-server-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mise.toml"),
            "[task_config]\nincludes = [\"mise.tasks.toml\"]\n",
        )
        .unwrap();
        let uri = format!("file://{}/mise.tasks.toml", dir.display());
        let replies = session(&[open(&uri, "[build]\nrun = \"x\"\nbogus = 1\n")]);
        assert_eq!(replies[0]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn other_toml_files_are_left_alone() {
        let uri = "file:///p/Cargo.toml";
        let replies = session(&[
            open(uri, "[package]\nname = 1\n"),
            json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/hover",
                "params": {"textDocument": {"uri": uri}, "position": {"line": 0, "character": 2}}}),
        ]);
        assert_eq!(replies[0]["params"]["diagnostics"], json!([]));
        assert_eq!(replies[1]["result"], Value::Null);
    }

    #[test]
    fn closing_a_document_clears_its_diagnostics() {
        let uri = "file:///p/mise.toml";
        let replies = session(&[
            open(uri, "a = \n"),
            json!({"jsonrpc": "2.0", "method": "textDocument/didClose",
                "params": {"textDocument": {"uri": uri}}}),
        ]);
        assert_eq!(
            replies[0]["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(replies[1]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn recognizes_config_files() {
        for yes in [
            "file:///p/mise.toml",
            "file:///p/.mise.toml",
            "file:///p/mise.local.toml",
            "file:///p/.mise.test.toml",
            "file:///h/.config/mise/config.toml",
            "file:///p/.mise/config.toml",
            "file:///h/.config/mise/conf.d/a.toml",
            "file:///h/My%20Dir/mise.toml",
        ] {
            assert!(is_mise_config(yes), "{yes}");
        }
        for no in [
            "file:///p/Cargo.toml",
            "file:///p/config.toml",
            "file:///p/.miserc.toml",
            "file:///p/mise.json",
            "untitled:Untitled-1",
        ] {
            assert!(!is_mise_config(no), "{no}");
        }
    }
}
