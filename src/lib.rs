use std::default::Default;
use std::fmt;
use std::io::{self, prelude::*};
use std::process::Command;

use log::*;
use lsp_types::notification::Notification as LspNotification;
use lsp_types::request::Request as LspRequest;
use lsp_types::{
    lsp_notification, lsp_request, ExecuteCommandOptions, InitializeResult, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    WorkDoneProgressOptions,
};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

const OPEN_PREVIEW_COMMAND: &str = "Open Preview";

pub mod protocol;

use protocol::{
    LspTransport, Message, Notification, ProtocolError, Request, Response, ResponseError, ResultExt,
};

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    /// Auto-open the preview.
    auto: bool,

    /// Program and arguments to use to open the preview. If `None`, use the default browser.
    browser: Option<(String, Vec<String>)>,

    /// highlight.js style to use for syntax highlighting in code blocks.
    theme: String,

    /// Serve static files like images. This should only be use with trusted documents.
    serve_static: bool,

    /// Program and arguments to use to render the markdown. If `None`, use the default renderer.
    renderer: Option<(String, Vec<String>)>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            auto: true,
            browser: None,
            theme: String::from("github"),
            serve_static: false,
            renderer: None,
        }
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Settings {
            markdown: Option<Markdown>,
        }

        #[derive(Deserialize)]
        struct Markdown {
            preview: Option<Preview>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Preview {
            auto: Option<bool>,
            #[serde(deserialize_with = "deserialize_opt_command")]
            #[serde(default)]
            browser: Option<(String, Vec<String>)>,
            code_theme: Option<String>,
            serve_static: Option<bool>,
            #[serde(deserialize_with = "deserialize_opt_command")]
            #[serde(default)]
            renderer: Option<(String, Vec<String>)>,
        }

        Settings::deserialize(deserializer).map(|settings| {
            let preview_settings = settings.markdown.and_then(|markdown| markdown.preview);

            let mut settings = crate::Settings::default();

            if let Some(preview_settings) = preview_settings {
                if let Some(auto) = preview_settings.auto {
                    settings.auto = auto;
                }

                if let Some(theme) = preview_settings.code_theme {
                    settings.theme = theme;
                }

                settings.browser = preview_settings.browser;

                if let Some(serve_static) = preview_settings.serve_static {
                    settings.serve_static = serve_static;
                }

                settings.renderer = preview_settings.renderer;
            }

            settings
        })
    }
}

pub struct Server<R, W> {
    transport: LspTransport<R, W>,
    settings: Settings,
    shutdown: bool,
    markdown_server: aurelius::Server,
    /// True if the server is being run as part of a test. The preview will not be spawned.
    #[doc(hidden)]
    pub test: bool,
}

impl<R, W> Server<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(reader: R, writer: W) -> Self {
        let server = aurelius::Server::bind("localhost:0").unwrap();

        let mut settings = Settings::default();

        // Act as if auto-open wsas previously set to false, so that the preview will open on the
        // first configuration change if auto is set to true.
        settings.auto = false;

        Server {
            transport: LspTransport::new(reader, writer),
            settings,
            shutdown: false,
            markdown_server: server,
            test: false,
        }
    }

    pub fn serve(mut self) -> io::Result<()> {
        loop {
            let message = match self.transport.decode() {
                Ok(Some(message)) => message,
                Ok(None) => return Ok(()),
                Err(ProtocolError::Io(err)) => return Err(err),
                Err(err) => {
                    let code = match err {
                        ProtocolError::HTTP(..)
                        | ProtocolError::MissingContentLength
                        | ProtocolError::InvalidContentLength => -32700,
                        ProtocolError::InvalidJson(..) => -32600,
                        ProtocolError::Io(..) => unimplemented!("I/O errors handled above"),
                    };
                    let response = Message::error(ResponseError {
                        code,
                        message: err.to_string(),
                        data: None,
                    });

                    self.transport.encode(&response)?;

                    continue;
                }
            };

            match message {
                Message::Request(req) => {
                    let res = self.handle_request(req);
                    self.transport.encode(&Message::Response(res))?;
                }
                Message::Notification(not)
                    if not.method.as_str() == <lsp_notification!("exit")>::METHOD =>
                {
                    return Ok(())
                }
                Message::Notification(not) => self.handle_notification(not),
                Message::Response(res) => unimplemented!("unhandled response: {:?}", res),
            }
        }
    }

    fn handle_request(&mut self, req: Request) -> Response {
        match req.method.as_str() {
            <lsp_request!("initialize")>::METHOD => Ok(InitializeResult {
                capabilities: ServerCapabilities {
                    text_document_sync: Some(TextDocumentSyncCapability::Options(
                        TextDocumentSyncOptions {
                            open_close: Some(true),
                            change: Some(TextDocumentSyncKind::Full),
                            ..Default::default()
                        },
                    )),
                    execute_command_provider: Some(ExecuteCommandOptions {
                        commands: vec![String::from(OPEN_PREVIEW_COMMAND)],
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                    }),
                    ..Default::default()
                },
                server_info: Some(ServerInfo {
                    name: String::from(env!("CARGO_PKG_NAME")),
                    version: Some(String::from(env!("CARGO_PKG_VERSION"))),
                }),
            })
            .into_response(req.id),
            <lsp_request!("workspace/executeCommand")>::METHOD => {
                let params =
                    <lsp_request!("workspace/executeCommand") as LspRequest>::Params::deserialize(
                        req.params.unwrap_or(Value::Null),
                    )
                    .unwrap();

                match &*params.command {
                    OPEN_PREVIEW_COMMAND => {
                        if let Err(e) = self.open_preview() {
                            return Err::<Value, _>(ResponseError {
                                code: 1,
                                message: format!("could not open preview: {}", e),
                                data: None,
                            })
                            .into_response(req.id);
                        }
                    }
                    _ => info!("unknown command: {}", params.command),
                }

                Ok(Value::Null).into_response(req.id)
            }
            <lsp_request!("shutdown")>::METHOD => {
                self.shutdown = true;
                Ok(Value::Null).into_response(req.id)
            }
            method => {
                info!("unsupported request method: {}", method);
                Ok(Value::Null).into_response(req.id)
            }
        }
    }

    fn handle_notification(&mut self, not: Notification) {
        match not.method.as_str() {
            <lsp_notification!("workspace/didChangeConfiguration")>::METHOD => {
                let params = <lsp_notification!("workspace/didChangeConfiguration") as LspNotification>::Params::deserialize(
                    not.params.unwrap(),
                ).unwrap();

                if let Ok(settings) = Settings::deserialize(params.settings) {
                    info!("changed configuration: {:?}", settings);

                    let old_auto_setting = self.settings.auto;

                    self.settings = settings;

                    if self.settings.auto && !old_auto_setting {
                        if let Err(e) = self.open_preview() {
                            error!("could not open browser: {}", e);
                        }
                    }

                    self.markdown_server
                        .set_highlight_theme(self.settings.theme.clone());

                    // There is currently no way to unset the static root wihout restarting the browser
                    if self.settings.serve_static {
                        self.markdown_server
                            .set_static_root(std::env::current_dir().unwrap())
                    }

                    if let Some(renderer) = &self.settings.renderer {
                        let mut command = Command::new(&renderer.0);
                        command.args(&renderer.1);
                        self.markdown_server.set_external_renderer(command)
                    }
                }
            }
            <lsp_notification!("textDocument/didOpen")>::METHOD => {
                let params =
                    <lsp_notification!("textDocument/didOpen") as LspNotification>::Params::deserialize(
                        not.params.unwrap(),
                    )
                    .unwrap();

                self.markdown_server
                    .send(params.text_document.text)
                    .unwrap();
            }
            <lsp_notification!("textDocument/didChange")>::METHOD => {
                let params =
                    <lsp_notification!("textDocument/didChange") as LspNotification>::Params::deserialize(
                        not.params.unwrap(),
                    )
                    .unwrap();

                let mut content_changes = params.content_changes;

                assert_eq!(content_changes.len(), 1);

                self.markdown_server
                    .send(content_changes.remove(0).text)
                    .unwrap();
            }
            <lsp_notification!("exit")>::METHOD => unreachable!("handled by caller"),
            method => info!("unimplemented notification method: {}", method),
        }
    }

    fn open_preview(&mut self) -> io::Result<()> {
        if self.test {
            return Ok(());
        }

        if let Some((name, args)) = &mut self.settings.browser {
            let mut command = Command::new(name);
            command.args(args);
            self.markdown_server.open_specific_browser(command)
        } else {
            self.markdown_server.open_browser()
        }
    }
}

fn deserialize_command<'de, D>(deserializer: D) -> Result<(String, Vec<String>), D::Error>
where
    D: Deserializer<'de>,
{
    struct CommandVisitor;

    impl<'de> Visitor<'de> for CommandVisitor {
        type Value = (String, Vec<String>);

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "string or array of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok((String::from(value), vec![]))
        }

        fn visit_seq<S>(self, seq: S) -> Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let args = <Vec<String>>::deserialize(de::value::SeqAccessDeserializer::new(seq))?;
            let (program, args) = args
                .split_first()
                .ok_or_else(|| de::Error::invalid_length(0, &"at least a program name"))?;

            Ok((String::from(program), args.to_vec()))
        }
    }

    deserializer.deserialize_any(CommandVisitor)
}

fn deserialize_opt_command<'de, D>(
    deserializer: D,
) -> Result<Option<(String, Vec<String>)>, D::Error>
where
    D: Deserializer<'de>,
{
    // serde#723
    #[derive(Deserialize)]
    struct Wrapper(#[serde(deserialize_with = "deserialize_command")] (String, Vec<String>));

    let v = Option::deserialize(deserializer)?;
    Ok(v.map(|Wrapper(command)| command))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde::Deserialize;
    use serde_json::json;

    use super::Settings;

    #[test]
    fn deserialize_empty_settings() -> Result<(), Box<dyn Error>> {
        let json = json!({});

        assert_eq!(Settings::deserialize(json)?, Settings::default());

        Ok(())
    }

    #[test]
    fn deserialize_empty_markdown_settings() -> Result<(), Box<dyn Error>> {
        let json = json!({
            "markdown": {}
        });

        assert_eq!(Settings::deserialize(json)?, Settings::default());

        Ok(())
    }

    #[test]
    fn deserialize_empty_preview_settings() -> Result<(), Box<dyn Error>> {
        let json = json!({
            "markdown": {
                "preview": {}
            }
        });

        assert_eq!(Settings::deserialize(json)?, Settings::default());

        Ok(())
    }

    #[test]
    fn deserialize_settings() -> Result<(), Box<dyn Error>> {
        let json = json!({
            "markdown": {
                "preview": {
                    "auto": false,
                    "browser": "firefox"
                }
            }
        });

        let settings = Settings::deserialize(json)?;

        assert_eq!(settings.auto, false);
        assert_eq!(settings.browser, Some((String::from("firefox"), vec![])));

        Ok(())
    }

    #[test]
    fn deserialize_browser_list() -> Result<(), Box<dyn Error>> {
        let json = json!({
            "markdown": {
                "preview": {
                    "browser": ["open", "-g"]
                }
            }
        });

        let settings = Settings::deserialize(json)?;

        assert_eq!(
            settings.browser,
            Some((String::from("open"), vec![String::from("-g")]))
        );

        Ok(())
    }

    #[test]
    fn deserialize_theme() -> Result<(), Box<dyn Error>> {
        let json = json!({
            "markdown": {
                "preview": {
                    "codeTheme": "darcula"
                }
            }
        });

        let settings = Settings::deserialize(json)?;

        assert_eq!(settings.theme, "darcula");

        Ok(())
    }
}
