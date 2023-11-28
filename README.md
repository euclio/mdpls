# mdpls: Markdown Preview Language Server

> Markdown, please!

mdpls is a [language server] that provides a live HTML preview of your markdown
in your browser.

mdpls is powered by [aurelius], which also powers [vim-markdown-composer].

### Installation

mdpls requires stable Rust, which can easily be installed and updated via
[rustup].

```sh
cargo install --git https://github.com/euclio/mdpls
```

The `mdpls` binary will be installed to `.cargo/bin` in your home directory.

### Usage

mdpls works with your [favorite editor's LSP plugin][lsp-tools]. Consult
your plugin's documentation for information on how to configure a new language
server.

Like most language servers, mdpls operates over stdin and stdout.

### Configuration

| Setting | Type | Description | Default
| - | - | - | -
| `markdown.preview.auto` | boolean | Open the markdown preview automatically. | `true`
| `markdown.preview.browser` | array or string | The program and arguments to use for opening the preview window. If not specified, the user's default browser will be used. The preview URL will be appended to this program as an argument. | None
| `markdown.preview.codeTheme` | string | [highlight.js style] to use for syntax highlighting in code blocks. | `github`
| `markdown.preview.serveStatic` | boolean | Serve static files like images (this should only be use with trusted documents) | `false`
| `markdown.preview.renderer` | array or string | The program to use to render the markdown to html. If not specified, the builtin markdown renderer will be used. | None
| `markdown.preview.deferUpdates.ms_before` | int | After the document changes, how long to wait before updating the preview | 0
| `markdown.preview.deferUpdates.ms_between` | int | Between two document changes, how long to wait before updating the preview (200ms -> up to 5 updates per second) | 0

Setting either `deferUpdates.ms_before` or `deferUpdates.ms_between` to a nonzero value enables enables the deferUpdates mode. Here, the preview is updated slower and less frequently (instead of updating every time any change to the document is made) to preserve battery and improve usability in large documents. This mode requires spawning an additional thread.

### Commands

The language server also provides commands for interacting with the browser preview.

| Command | Description
| - | -
| `Open Preview` | Opens the markdown preview.

[language server]: https://microsoft.github.io/language-server-protocol/
[aurelius]: https://github.com/euclio/aurelius
[vim-markdown-composer]: https://github.com/euclio/vim-markdown-composer
[rustup]: https://rustup.rs
[lsp-tools]: https://microsoft.github.io/language-server-protocol/implementors/tools/
[highlight.js style]: https://highlightjs.org/static/demo/
