use std::error::Error;
use std::io;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;
use lsp_types::{
    lsp_notification, lsp_request, ClientCapabilities, DidChangeConfigurationParams,
    InitializeParams,
};
use mdpls::protocol::{LspTransport, Message, Notification, Request};
use serde_json::{json, Value};

struct Client {
    transport: LspTransport<ChildStdout, ChildStdin>,
    child: Child,
}

impl Client {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut command = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

        let mut child = command
            .arg("--test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

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
        transport.encode(&Message::Request(req))?;

        // Wait for initialized response.
        transport.decode().unwrap().unwrap();

        Ok(Client { transport, child })
    }

    fn did_change_configuration(&mut self, settings: Value) -> io::Result<()> {
        let did_change_config = Notification::new::<
            lsp_notification!("workspace/didChangeConfiguration"),
        >(Some(DidChangeConfigurationParams { settings }));

        self.transport
            .encode(&Message::Notification(did_change_config))?;

        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let exit_notification = Notification::new::<lsp_notification!("exit")>(None);
        self.transport
            .encode(&Message::Notification(exit_notification))
            .unwrap();

        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn bad_type() -> Result<(), Box<dyn Error>> {
    let mut client = Client::new()?;

    client.did_change_configuration(json!({
        "markdown": {
            "preview": {
                "auto": 1337
            }
        }
    }))?;

    Ok(())
}
