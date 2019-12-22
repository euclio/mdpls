use std::io::{self, prelude::*};

use atoi::atoi;
use httparse::{Status, EMPTY_HEADER};
use log::*;
use lsp_types::notification::Notification as LspNotification;
use lsp_types::request::Request as LspRequest;
use serde::de::{self, Unexpected};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

const MAX_HEADERS: usize = 4;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Could not parse HTTP: {0}")]
    HTTP(#[from] httparse::Error),

    #[error("Missing Content-Length header")]
    MissingContentLength,

    #[error("Content-Length was not a number")]
    InvalidContentLength,

    #[error("Invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

pub trait ResultExt {
    fn into_response(self, id: Value) -> Response;
}

impl<T> ResultExt for Result<T, ResponseError>
where
    T: Serialize,
{
    fn into_response(self, id: Value) -> Response {
        let (result, error) = match self {
            Ok(val) => (
                Some(serde_json::to_value(val).expect("could not serialize Value to json")),
                None,
            ),
            Err(err) => (None, Some(err)),
        };

        Response { id, result, error }
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

impl Request {
    pub fn new<R>(id: Value, params: Option<R::Params>) -> Self
    where
        R: LspRequest,
        R::Params: Serialize,
    {
        Request {
            id,
            method: String::from(R::METHOD),
            params: params
                .map(|params| serde_json::to_value(params).expect("error serializing LSP type")),
        }
    }
}

#[derive(Debug)]
pub struct Notification {
    pub method: String,
    pub params: Option<Value>,
}

impl Notification {
    pub fn new<N>(params: Option<N::Params>) -> Self
    where
        N: LspNotification,
        N::Params: Serialize,
    {
        Notification {
            method: String::from(N::METHOD),
            params: params
                .map(|params| serde_json::to_value(params).expect("error serializing LSP type")),
        }
    }
}

#[derive(Debug)]
pub struct Response {
    pub id: Value,
    result: Option<Value>,
    error: Option<ResponseError>,
}

impl Response {
    pub fn into_result(self) -> Result<Value, ResponseError> {
        match (self.result, self.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(error),
            (result, error) => panic!(
                "expected exactly one of result: {:?}, error: {:?}",
                result, error
            ),
        }
    }
}

#[derive(Debug)]
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
}

impl Message {
    pub fn error(err: ResponseError) -> Self {
        Message::Response(Response {
            id: Value::Null,
            result: None,
            error: Some(err),
        })
    }
}

impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;

        map.serialize_entry("jsonrpc", "2.0")?;

        match self {
            Message::Request(req) => {
                map.serialize_entry("id", &req.id)?;
                map.serialize_entry("method", &req.method)?;

                if let Some(params) = &req.params {
                    map.serialize_entry("params", params)?;
                }
            }
            Message::Response(res) => {
                map.serialize_entry("id", &res.id)?;

                if let Some(result) = &res.result {
                    map.serialize_entry("result", result)?;
                } else if let Some(error) = &res.error {
                    map.serialize_entry("error", error)?;
                }
            }
            Message::Notification(not) => {
                map.serialize_entry("method", &not.method)?;

                if let Some(params) = &not.params {
                    map.serialize_entry("params", params)?;
                }
            }
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct RawMessage {
            jsonrpc: String,
            #[serde(default, deserialize_with = "double_option")]
            id: Option<Value>,
            method: Option<String>,
            #[serde(default, deserialize_with = "double_option")]
            params: Option<Value>,
            #[serde(default, deserialize_with = "double_option")]
            result: Option<Value>,
            #[serde(default, deserialize_with = "double_option")]
            error: Option<ResponseError>,
        }

        fn double_option<'de, T, D>(de: D) -> Result<Option<T>, D::Error>
        where
            T: Deserialize<'de>,
            D: Deserializer<'de>,
        {
            Deserialize::deserialize(de).map(Some)
        }

        let val = RawMessage::deserialize(deserializer)?;

        if val.jsonrpc != "2.0" {
            return Err(de::Error::invalid_value(
                Unexpected::Other("JSON-RPC protocol version"),
                &"2.0",
            ));
        }

        assert_eq!(val.jsonrpc, "2.0");

        let message = if val.result.is_some() || val.error.is_some() {
            if val.result.is_some() && val.error.is_some() {
                return Err(de::Error::custom("expected exactly one of result or error"));
            }

            Message::Response(Response {
                id: val.id.ok_or_else(|| de::Error::missing_field("id"))?,
                result: val.result,
                error: val.error,
            })
        } else {
            let params = val.params;

            let method = val
                .method
                .ok_or_else(|| de::Error::missing_field("method"))?;

            if let Some(id) = val.id {
                Message::Request(Request { id, method, params })
            } else {
                Message::Notification(Notification { method, params })
            }
        };

        Ok(message)
    }
}

pub struct LspTransport<R, W> {
    reader: buf_redux::BufReader<R>,
    writer: W,
}

impl<R, W> LspTransport<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(reader: R, writer: W) -> Self {
        LspTransport {
            reader: buf_redux::BufReader::new(reader),
            writer,
        }
    }

    pub fn encode(&mut self, message: &Message) -> io::Result<()> {
        let json = serde_json::to_string(&message).expect("unserializable message");

        trace!("<- {}", json);

        write!(self.writer, "Content-Length: {}\r\n", json.len())?;
        write!(self.writer, "\r\n")?;
        self.writer.write_all(json.as_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    pub fn decode(&mut self) -> Result<Option<Message>, ProtocolError> {
        let (header_bytes, content_length) = loop {
            let buf = self.reader.fill_buf()?;

            if buf.is_empty() {
                return Ok(None);
            }

            let mut headers = [EMPTY_HEADER; MAX_HEADERS];

            match httparse::parse_headers(&buf, &mut headers)? {
                Status::Partial => {
                    self.reader.read_into_buf()?;
                }
                Status::Complete((n, parsed)) => {
                    let content_length_header = parsed
                        .iter()
                        .find(|header| header.name == "Content-Length")
                        .ok_or_else(|| ProtocolError::MissingContentLength)?;

                    break (
                        n,
                        atoi(content_length_header.value)
                            .ok_or_else(|| ProtocolError::InvalidContentLength)?,
                    );
                }
            }
        };

        self.reader.consume(header_bytes);

        let mut json_buf = vec![0; content_length];

        self.reader.read_exact(&mut json_buf)?;

        trace!("-> {}", String::from_utf8_lossy(&json_buf));

        Ok(Some(serde_json::from_slice(&json_buf)?))
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::{self, Read};

    use assert_matches::assert_matches;
    use serde::Deserialize;
    use serde_json::{self, json, Value};

    use super::{LspTransport, Message, ProtocolError};

    #[test]
    fn deseialize_request_string_id() {
        let json = json!({ "jsonrpc": "2.0", "id": "1", "method": "foo" });

        let request = assert_matches!(Message::deserialize(json), Ok(Message::Request(req)) => req);

        assert_eq!(request.id, json!("1"));
    }

    #[test]
    fn deseialize_request_missing_method() {
        let json = json!({ "jsonrpc": "2.0", "id": 1});

        let err = Message::deserialize(json).unwrap_err();

        assert!(err.to_string().contains("missing field `method`"));
    }

    #[test]
    fn deserialize_response() {
        let json = json!({ "jsonrpc": "2.0", "id": 1, "result": null});

        let response =
            assert_matches!(Message::deserialize(json), Ok(Message::Response(res)) => res);

        assert_eq!(response.into_result(), Ok(Value::Null));
    }

    #[test]
    fn deserialize_unknown_fields() {
        let json = json!({ "jsonrpc": "2.0", "id": 1, "result": null, "extra": 1 });

        let err = Message::deserialize(json).unwrap_err();

        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn deserialize_bad_version() {
        let json = json!({ "jsonrpc": "1.0" });

        let err = Message::deserialize(json).unwrap_err();

        assert!(err.to_string().contains("protocol version"));
    }

    #[test]
    fn deserialize_result_and_error() {
        let json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": null,
            "error": {
                "code": 1,
                "message": "error"
            }
        });

        let err = Message::deserialize(json).unwrap_err();

        assert!(err
            .to_string()
            .contains("expected exactly one of result or error"));
    }

    #[test]
    fn deserialize_response_missing_id() {
        let json = json!({ "jsonrpc": "2.0", "result": null });

        let err = Message::deserialize(json).unwrap_err();
        assert_eq!(err.to_string(), "missing field `id`");
    }

    #[test]
    fn deserialize_response_null_id() -> Result<(), Box<dyn Error>> {
        let json = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": 1,
            "message": "error",
        }});

        let response = assert_matches!(Message::deserialize(json)?, Message::Response(res) => res);
        assert_eq!(response.id, Value::Null);

        Ok(())
    }

    #[test]
    fn deserialize_notification_missing_method() {
        let json = json!({ "jsonrpc": "2.0" });

        let err = Message::deserialize(json).unwrap_err();

        assert!(err.to_string().contains("missing field `method`"));
    }

    #[test]
    fn decode_multiple_frames() {
        let frames = concat!(
            "Content-Length: 52\r\n\r\n",
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
            "Content-Length: 44\r\n\r\n",
            r#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#,
        );

        let mut transport = LspTransport::new(frames.as_bytes(), io::sink());

        transport.decode().unwrap().unwrap();
        transport.decode().unwrap().unwrap();
    }

    #[test]
    fn decode_short_read() {
        struct ShortReader<'a> {
            inner: &'a [u8],
            first_read: bool,
        }

        impl<'a> Read for ShortReader<'a> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.first_read {
                    self.first_read = false;
                    self.inner.read_exact(&mut buf[..20])?;
                    Ok(20)
                } else {
                    self.inner.read(buf)
                }
            }
        }

        let frame = concat!(
            "Content-Length: 38\r\n\r\n",
            r#"{"jsonrpc":"2.0","id":1,"result":null}"#
        );

        let reader = ShortReader {
            inner: frame.as_bytes(),
            first_read: true,
        };

        let mut transport = LspTransport::new(reader, io::sink());

        transport.decode().unwrap();
    }

    #[test]
    fn decode_eof() {
        let frame: &[u8] = &[];
        let mut transport = LspTransport::new(frame, io::sink());

        transport.decode().unwrap();
    }

    #[test]
    fn decode_missing_content_length() {
        let frame = concat!(
            "Content-Type: application/vscode-jsonrpc; charset=utf8\r\n\r\n",
            r#"{"jsonrpc":"2.0","id":1,"result":null}"#
        );
        let mut transport = LspTransport::new(frame.as_bytes(), io::sink());

        let err = transport.decode().unwrap_err();

        assert_matches!(err, ProtocolError::MissingContentLength);
    }

    #[test]
    fn decode_invalid_content_length() {
        let frame = concat!(
            "Content-Length: not a number\r\n\r\n",
            r#"{"jsonrpc":"2.0","id":1,"result":null}"#
        );
        let mut transport = LspTransport::new(frame.as_bytes(), io::sink());

        let err = transport.decode().unwrap_err();

        assert_matches!(err, ProtocolError::InvalidContentLength);
    }

    #[test]
    fn decode_invalid_header() {
        let frame = concat!(
            "Internal Whitespace: yes\r\n\r\n",
            r#"{"jsonrpc":"2.0","id":1,"result":null}"#
        );
        let mut transport = LspTransport::new(frame.as_bytes(), io::sink());

        let err = transport.decode().unwrap_err();

        assert_matches!(err, ProtocolError::HTTP(httparse::Error::HeaderName));
    }

    #[test]
    fn decode_invalid_json() {
        let frame = concat!(
            "Content-Length: 38\r\n\r\n",
            r#"{"jsonrpc":"1.0","id":1,"result":null}"#
        );

        let mut transport = LspTransport::new(frame.as_bytes(), io::sink());

        let err = transport.decode().unwrap_err();

        assert_matches!(err, ProtocolError::InvalidJson(_));
    }
}
