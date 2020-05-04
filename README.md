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
