# 编辑器与工具链集成

> 本文说明如何在编辑器里获得 Ada-D 的**语法高亮**与**实时诊断**，以及如何在 CI 里做静态检查。
> 诊断的语义以 [SPEC_RUNTIME §8](SPEC_RUNTIME.md)（错误模型与位置）为准，本文不重复规范。

---

## 1. 三个入口

| 入口 | 用途 | 是否执行脚本 |
| :--- | :--- | :--- |
| `ada check <file>` | CI / pre-commit / 编辑器保存钩子：判断"能不能编译" | ❌ 不执行 |
| `ada-lsp` | 编辑器的 LSP 服务器：实时波浪线 | ❌ 不执行 |
| `ada run <file>` | 真正运行（含运行时错误） | ✅ 执行 |

三者共享同一份诊断来源：`Engine::check`（解析 + 编译）与 `Engine::eval_diagnostic`（执行）。
因此编辑器里的报错与终端里的报错**不会打架**——同一条错误消息、同一个 `line:col`。

```bash
# CI 里最常用的一行
ada check scripts/*.ada
```

- 成功时**保持安静**且退出码 `0`（Unix 惯例，便于脚本组合）。
- 失败时输出单行 `file:line:col: error: <消息>` 并退出码 `1`。

---

## 2. LSP 服务器

`crates/ada_lsp` 提供最小可用的 LSP 服务器（二进制名 `ada-lsp`），在 stdin/stdout 上按
`Content-Length` 分帧收发 JSON-RPC。

| 方法 | 方向 | 处理 |
| :--- | :--- | :--- |
| `initialize` | 请求 | 回 `capabilities.textDocumentSync = 1`（全量同步）+ `serverInfo` |
| `initialized` | 通知 | 忽略 |
| `textDocument/didOpen` | 通知 | 记录文本 → 发布诊断 |
| `textDocument/didChange` | 通知 | 用最后一次 change 的全文替换 → 发布诊断 |
| `textDocument/didClose` | 通知 | 丢弃文本 → 发布空诊断（清掉波浪线） |
| `shutdown` | 请求 | 回 `null` |
| `exit` | 通知 | 结束进程（未先 `shutdown` 则以非零码退出，符合规范） |
| 其他 | — | 请求回 `-32601`；通知忽略 |

诊断以 `textDocument/publishDiagnostics` 推送，字段：`range`（0 基行列）、`severity = 1`、
`source = "ada"`、`message`（与 CLI 同一份消息）。

### 2.1 只做静态诊断（刻意的设计）

LSP **不执行脚本**。编辑器里每次输入都会触发诊断，若真的运行脚本，`sleep_ms`、宿主能力、
沙箱限额都会影响编辑器，还有副作用。因此：

- ✅ 覆盖：词法错误、语法错误、编译错误（含 `let` 不可变赋值这类最常见的 AI 失误）。
- ❌ 不覆盖：运行时错误（未定义全局变量、类型错误、除零……）——这些需要执行才知道，
  请用 `ada run`。

### 2.2 手动试一下

```bash
# 造一个语法错误
printf 'let x = (1 +\n' > /tmp/bad.ada

# 服务器会对 didOpen 回一条 publishDiagnostics
printf 'Content-Length: 92\r\n\r\n{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///tmp/bad.ada","text":"let x = (1 +"}}}' | ada-lsp
```

---

## 3. VSCode

扩展位于 [editors/vscode](../editors/vscode/README.md)，包含：

- `syntaxes/ada.tmLanguage.json`：语法高亮（关键字/插值/注释/数字/运算符）。
- `language-configuration.json`：括号与引号自动闭合、缩进。
- `extension.js` + `lsp-client.js`：零依赖 LSP 客户端，驱动 `ada-lsp` 显示诊断。

安装：把目录拷到 `~/.vscode/extensions/ada-d-0.1.0/`（Windows 为 `%USERPROFILE%\.vscode\extensions\`）
后重启 VSCode。若 `ada-lsp` 不在 `PATH`，在设置里填 `ada.lsp.path` 指向二进制路径：

```bash
cargo build --release -p ada_lsp     # 产物：target/release/ada-lsp(.exe)
```

---

## 4. 其他编辑器（通用 LSP 配置）

任何支持 LSP 的编辑器都能接：启动命令 `ada-lsp --stdio`，语言 id `ada`，文件后缀 `.ada`。

- **Neovim**（`vim.lsp.start`）：`cmd = { "ada-lsp", "--stdio" }`，`filetypes = { "ada" }`
  （`ada` 文件类型可用 `vim.filetype.add({ extension = { ada = "ada" } })` 注册）。
- **Emacs**（Eglot）：`(add-to-list 'eglot-server-programs '(ada-mode . ("ada-lsp" "--stdio")))`。
- **Helix**：在 `languages.toml` 里 `[[language]] name = "ada"` 配 `language-servers = ["ada-lsp"]`。

---

## 5. 哪些测试守护这条链路

| 关注点 | 测试 |
| :--- | :--- |
| 诊断内容与位置（静态检查） | `crates/ada_engine/tests/diagnostics.rs`、`crates/ada_lsp/src/lib.rs` 单元测试 |
| LSP 协议逻辑（消息 → 响应/通知） | `crates/ada_lsp/src/lib.rs` 单元测试（含畸形输入不 panic） |
| 真实进程的分帧会话 | `crates/ada_lsp/tests/session.rs`（起 `ada-lsp`，initialize → didOpen → didChange → shutdown/exit） |
| **随扩展发布的 JS 客户端** ↔ 服务器互通 | `crates/ada_lsp/tests/node_client.rs`（用 `editors/vscode/lsp-client.js` 驱动真实二进制） |
| 语法高亮与语言关键字不漂移 | `crates/ada_cli/tests/editor_grammar.rs`（与 `ada_lexer::KEYWORDS`/`OPERATORS` 集合比对） |
| CLI 错误格式契约 | `crates/ada_cli/tests/cli_errors.rs` |

> 互通测试需要 `node`；缺失时会打印说明并跳过（CI 的 ubuntu 镜像自带）。

---

## 6. 明确的非目标（0.1.0）

- 补全、跳转定义、重命名、格式化、语义高亮。
- 增量同步（当前是全量同步，脚本文件很小，代价可忽略）。
- 运行时诊断与调试器（需要执行或插桩，另行设计）。
- 代码片段（snippets）与包管理集成。
