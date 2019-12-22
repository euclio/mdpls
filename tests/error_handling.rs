use std::error::Error;
use std::io::prelude::*;
use std::io::BufReader;
use std::process::{Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;
use assert_matches::assert_matches;

use mdpls::protocol::{LspTransport, Message, ResponseError};

#[test]
fn not_http() -> Result<(), Box<dyn Error>> {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());

    write!(stdin, "not http")?;
    stdin.flush()?;

    let mut transport = LspTransport::new(stdout, stdin);

    let message = transport.decode()?;

    let res = assert_matches!(message, Some(Message::Response(res)) => res);

    assert_eq!(
        res.into_result(),
        Err(ResponseError {
            code: -32700,
            message: String::from("Could not parse HTTP: invalid header name"),
            data: None,
        })
    );

    drop(transport);

    assert!(!child.wait()?.success());

    Ok(())
}

#[test]
fn not_json() -> Result<(), Box<dyn Error>> {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());

    write!(stdin, "Content-Length: 8\r\n\r\n")?;
    write!(stdin, "not json")?;
    stdin.flush()?;

    let mut transport = LspTransport::new(stdout, stdin);

    let message = transport.decode()?;

    let res = assert_matches!(message, Some(Message::Response(res)) => res);

    assert_eq!(
        res.into_result(),
        Err(ResponseError {
            code: -32600,
            message: String::from("Invalid JSON: expected ident at line 1 column 2"),
            data: None,
        })
    );

    drop(transport);

    assert!(child.wait()?.success());

    Ok(())
}
