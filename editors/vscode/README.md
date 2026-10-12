# Ada-D VSCode 扩展（语法高亮 + 实时诊断）

| 文件 | 作用 |
| :--- | :--- |
| `syntaxes/ada.tmLanguage.json` | 语法高亮：关键字、字符串插值 `$"{...}"`、注释、数字、运算符、标点 |
| `language-configuration.json` | 括号/引号自动闭合、缩进规则 |
| `lsp-client.js` | **零依赖** LSP 客户端核心（起 `ada-lsp`、Content-Length 分帧、诊断回调），不引用 `vscode`，因此可在编辑器之外被测试 |
| `extension.js` | 编辑器胶水层：把 `publishDiagnostics` 映射成 VSCode 的 `DiagnosticCollection` |
| `package.json` | 扩展清单（语言、语法、`ada.lsp.path` 配置项） |

## 使用

### 1. 准备 `ada-lsp`

```bash
cargo build --release -p ada_lsp      # 产物：target/release/ada-lsp(.exe)
```

把该二进制放进 `PATH`，或在 VSCode 设置里指定：

```json
{ "ada.lsp.path": "/绝对路径/ada-lsp" }
```

### 2. 安装扩展

- **本地目录（最快）**：把本目录拷到
  `%USERPROFILE%\.vscode\extensions\ada-d-0.1.0\`（macOS/Linux：`~/.vscode/extensions/ada-d-0.1.0/`），
  重启 VSCode。
- **打包 vsix**：

  ```bash
  npx @vscode/vsce package     # 需要 Node.js
  code --install-extension ada-d-0.1.0.vsix
  ```

打开任意 `.ada` 文件：既能看到高亮，也会在保存/输入时看到诊断波浪线。

## 诊断的范围（重要）

诊断来自 `Engine::check`，**只做静态检查、不执行脚本**：

- ✅ 词法错误、语法错误、编译错误（含 `let` 不可变赋值这类常见失误）。
- ❌ 运行时错误（未定义全局变量、类型错误、除零……）——需要执行才知道，请在终端用 `ada run`。

这样设计是因为编辑器里每次输入都会触发诊断：执行脚本会有副作用，还可能被 `sleep_ms`
或沙箱限额拖住。完整说明见 [docs/EDITORS.md](../../docs/EDITORS.md)。

## 防漂移与验证

- 关键字/运算符列表**不允许**与语言实现脱节：`crates/ada_cli/tests/editor_grammar.rs`
  会解析 `syntaxes/ada.tmLanguage.json`，与 `ada_lexer::KEYWORDS` / `OPERATORS` 做**集合比对**
  （多一个少一个都失败），并校验 `package.json` 声明的路径真实存在。
- LSP 客户端**真的被端到端测过**：`crates/ada_lsp/tests/node_client.rs` 用本目录的
  `lsp-client.js` 驱动真实 `ada-lsp` 二进制，断言收到了语法/编译诊断且修好后清空。
- 只修改 JS 后可用 `node --check extension.js lsp-client.js` 做语法自检。

## 目前不含

- 补全、跳转、重命名、格式化、语义高亮。
- 增量同步（服务器声明全量同步；脚本文件很小，代价可忽略）。
