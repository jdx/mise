//! The LSP session: requests in, responses and diagnostics out.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::{Value, json};

use crate::complete::{self, Kind};
use crate::diagnostics::{self, Severity};
use crate::rpc::{read_message, write_message};
use crate::schema::Schema;
use crate::text::LineIndex;
use crate::{Formatter, Options, hover};

const METHOD_NOT_FOUND: i64 = -32601;

pub(crate) struct Server {
    schema: Schema,
    validator: OnceLock<Option<Validator>>,
    format: Option<Formatter>,
    version: String,
    documents: HashMap<String, String>,
}

impl Server {
    pub(crate) fn new(options: Options) -> Self {
        Self {
            schema: Schema::new(options.schema),
            validator: OnceLock::new(),
            format: options.format,
            version: options.version,
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
            "initialize" => Ok(self.initialize()),
            "shutdown" => Ok(Value::Null),
            "textDocument/completion" => Ok(self.completion(params)),
            "textDocument/hover" => Ok(self.hover(params)),
            "textDocument/formatting" => Ok(self.formatting(params)),
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
                self.documents.insert(uri.to_string(), text);
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
                self.documents.insert(uri.to_string(), text.to_string());
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
            "completionProvider": {"triggerCharacters": [".", "=", "[", "\""]},
            "hoverProvider": true,
        });
        if self.format.is_some() {
            capabilities["documentFormattingProvider"] = json!(true);
        }
        json!({
            "capabilities": capabilities,
            "serverInfo": {"name": "mise", "version": self.version},
        })
    }

    /// The text of an open mise config, or `None` for any other file: editors attach a TOML
    /// server to every TOML file, and `Cargo.toml` is not ours to judge.
    fn config(&self, uri: &str) -> Option<&str> {
        if !is_mise_config(uri) {
            return None;
        }
        self.documents.get(uri).map(String::as_str)
    }

    fn diagnostics(&self, uri: &str) -> Value {
        let Some(text) = self.config(uri) else {
            return json!([]);
        };
        let validator = self
            .validator
            .get_or_init(|| jsonschema::draft202012::new(self.schema.root()).ok())
            .as_ref();
        let index = LineIndex::new(text);
        let diagnostics: Vec<Value> = diagnostics::diagnose(text, &self.schema, validator)
            .into_iter()
            .map(|d| {
                let mut diagnostic = json!({
                    "range": range(&index, &d.range),
                    "severity": match d.severity {
                        Severity::Error => 1,
                        Severity::Warning => 2,
                    },
                    "source": "mise",
                    "message": d.message,
                });
                if d.deprecated {
                    diagnostic["tags"] = json!([2]);
                }
                diagnostic
            })
            .collect();
        json!(diagnostics)
    }

    fn completion(&self, params: &Value) -> Value {
        let Some((text, offset)) = self.cursor(params) else {
            return Value::Null;
        };
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
        let Some((text, offset)) = self.cursor(params) else {
            return Value::Null;
        };
        match hover::hover(text, offset, &self.schema) {
            Some(h) => json!({
                "contents": {"kind": "markdown", "value": h.markdown},
                "range": range(&LineIndex::new(text), &h.range),
            }),
            None => Value::Null,
        }
    }

    fn formatting(&self, params: &Value) -> Value {
        let (Some(format), Some(text)) = (
            &self.format,
            params["textDocument"]["uri"]
                .as_str()
                .and_then(|uri| self.config(uri)),
        ) else {
            return Value::Null;
        };
        match format(text) {
            Ok(formatted) if formatted != text => {
                let index = LineIndex::new(text);
                json!([{"range": range(&index, &(0..text.len())), "newText": formatted}])
            }
            // Unparseable text has no formatting; the diagnostics already say why.
            _ => json!([]),
        }
    }

    /// The document and byte offset a position request is about.
    fn cursor(&self, params: &Value) -> Option<(&str, usize)> {
        let text = self.config(params["textDocument"]["uri"].as_str()?)?;
        let line = params["position"]["line"].as_u64()? as usize;
        let character = params["position"]["character"].as_u64()? as usize;
        Some((text, LineIndex::new(text).offset(line, character)))
    }
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

/// Whether the file at `uri` is one mise loads as a config: `mise.toml`, `.mise.toml`,
/// `mise.<env>.toml`, `mise.local.toml`, `mise/config.toml`, `.config/mise/config.toml`, and
/// the files of a `conf.d` beside them.
fn is_mise_config(uri: &str) -> bool {
    let Some(path) = uri.strip_prefix("file://") else {
        return false;
    };
    let path = percent_decode(path);
    let mut parts = path.rsplit('/');
    let name = parts.next().unwrap_or_default();
    let parent = parts.next().unwrap_or_default();
    let grandparent = parts.next().unwrap_or_default();
    let in_mise_dir = |dir: &str| dir == "mise" || dir == ".mise";
    let Some(stem) = name.strip_suffix(".toml") else {
        return false;
    };
    let stem = stem.strip_prefix('.').unwrap_or(stem);
    let first = stem.split('.').next().unwrap_or_default();
    (first == "mise" && !stem.eq("miserc"))
        || (first == "config" && in_mise_dir(parent))
        || (parent == "conf.d" && in_mise_dir(grandparent))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = s
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
