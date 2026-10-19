//! LSP's base protocol: JSON messages framed by a `Content-Length` header.

use std::io::{self, BufRead, Write};

use serde_json::Value;

/// The next message, or `None` when the client closed the stream. A body that is not JSON is
/// skipped: its frame was consumed whole, so the stream is still in step and one bad message
/// should not end the session.
pub(crate) fn read_message(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            let line = line.trim_end();
            if line.is_empty() {
                if length.is_some() {
                    break;
                }
                continue;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let mut body = vec![0; length.unwrap_or_default()];
        input.read_exact(&mut body)?;
        if let Ok(message) = serde_json::from_slice(&body) {
            return Ok(Some(message));
        }
    }
}

pub(crate) fn write_message(output: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(&body)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_round_trip() {
        let mut buf = vec![];
        write_message(&mut buf, &json!({"id": 1, "method": "x"})).unwrap();
        write_message(&mut buf, &json!({"id": 2})).unwrap();
        let mut input = buf.as_slice();
        assert_eq!(
            read_message(&mut input).unwrap(),
            Some(json!({"id": 1, "method": "x"}))
        );
        assert_eq!(read_message(&mut input).unwrap(), Some(json!({"id": 2})));
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn a_malformed_body_is_skipped() {
        let raw = "Content-Length: 4\r\n\r\nnope\
                   Content-Length: 7\r\n\r\n{\"a\":1}";
        assert_eq!(
            read_message(&mut raw.as_bytes()).unwrap(),
            Some(json!({"a": 1}))
        );
    }

    #[test]
    fn other_headers_are_ignored() {
        let body = r#"{"a":1}"#;
        let raw = format!(
            "content-type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(
            read_message(&mut raw.as_bytes()).unwrap(),
            Some(json!({"a": 1}))
        );
    }
}
