use std::io::BufReader;
use std::process::{Command, Stdio};

use mdpls::protocol::{LspTransport, Message, Notification, Request};

use assert_cmd::cargo::CommandCargoExt;
use lsp_types::{
    lsp_notification, lsp_request, ClientCapabilities, InitializeParams, InitializedParams,
};
use serde_json::json;

#[test]
fn exit() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let mut child = cmd
        .arg("--test")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());

    let mut transport = LspTransport::new(stdout, stdin);

    #[allow(deprecated)]
    let req = Request::new::<lsp_request!("initialize")>(
        json!(0),
        Some(InitializeParams {
            process_id: None,
            root_path: None,
            capabilities: ClientCapabilities::default(),
            client_info: None,
            initialization_options: None,
            root_uri: None,
            trace: None,
            workspace_folders: None,
        }),
    );

    transport.encode(&Message::Request(req)).unwrap();

    let res = match transport.decode().unwrap().unwrap() {
        Message::Response(res) if res.id == json!(0) => res,
        message => panic!("unexpected message: {:?}", message),
    };

    res.into_result().unwrap();

    let not = Notification::new::<lsp_notification!("initialized")>(Some(InitializedParams {}));

    transport.encode(&Message::Notification(not)).unwrap();

    let shutdown_req = Request::new::<lsp_request!("shutdown")>(json!(1), None);
    transport.encode(&Message::Request(shutdown_req)).unwrap();

    let res = match transport.decode().unwrap().unwrap() {
        Message::Response(res) if res.id == json!(1) => res,
        message => panic!("unexpected message: {:?}", message),
    };

    res.into_result().unwrap();

    let exit_notification = Notification::new::<lsp_notification!("exit")>(None);
    transport
        .encode(&Message::Notification(exit_notification))
        .unwrap();

    assert!(child.wait().unwrap().success());

    let _ = transport;
}
