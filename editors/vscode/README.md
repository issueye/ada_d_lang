# Ada-D VSCode 扩展（语法高亮）

最小可用的编辑器支持：`.ada` 文件的关键字、字符串插值 `$"{...}"`、注释、数字与运算符高亮，
以及括号/引号自动闭合与缩进规则。

## 使用

### 方式一：本地扩展目录（最快）

1. 把本目录拷贝到 VSCode 扩展目录：
   - Windows：`%USERPROFILE%\.vscode\extensions\ada-d-0.1.0\`
   - macOS / Linux：`~/.vscode/extensions/ada-d-0.1.0/`
2. 重启 VSCode，打开任意 `.ada` 文件即可看到高亮。

### 方式二：打包成 vsix

```bash
npx @vscode/vsce package     # 需要 Node.js
code --install-extension ada-d-0.1.0.vsix
```

## 防漂移

语法文件里的关键字与运算符列表**不允许**与语言实现脱节：
`crates/ada_cli/tests/editor_grammar.rs` 会解析本目录的
`syntaxes/ada.tmLanguage.json`，并与 `crates/ada_lexer/src/lib.rs` 中的
`KEYWORDS` / `OPERATORS` 做**集合比对**（多一个少一个都会失败）。
`package.json` 里声明的路径也必须真实存在。

## 目前不含

- 语义诊断（LSP）：任务 T7.2 尚未实现；现阶段请在终端运行 `ada file.ada`
  查看 `file:line:col: error: <消息>` 形式的错误（T0.6 已统一格式）。
- 补全/跳转/格式化。
