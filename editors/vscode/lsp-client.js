// Ada-D 的零依赖 LSP 客户端核心（开发计划 T7.2）。
//
// 这里只做"传输 + 会话"：起 `ada-lsp` 子进程、按 Content-Length 分帧收发 JSON-RPC、
// 把 publishDiagnostics 回调出去。**不引用 `vscode`**，因此它既可以被编辑器扩展使用，
// 也可以在 VSCode 之外被真实测试（见 crates/ada_lsp/tests/node_client.rs）。

const { spawn } = require("child_process");

function encode(message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  return Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii"),
    body,
  ]);
}

function createClient(options) {
  const {
    command,
    args = [],
    onDiagnostics = () => {},
    onLog = () => {},
  } = options;

  const proc = spawn(command, [...args, "--stdio"], {
    stdio: ["pipe", "pipe", "pipe"],
  });

  let buffer = Buffer.alloc(0);
  let nextId = 1;
  let disposed = false;
  const waiters = new Map(); // uri -> { resolve, timer }

  function notify(uri, diagnostics) {
    onDiagnostics(uri, diagnostics);
    const waiter = waiters.get(uri);
    if (waiter) {
      waiters.delete(uri);
      clearTimeout(waiter.timer);
      waiter.resolve(diagnostics);
    }
  }

  function handle(message) {
    if (message.method === "textDocument/publishDiagnostics") {
      notify(message.params.uri, message.params.diagnostics || []);
    } else if (message.error) {
      onLog(`ada-lsp 错误响应: ${JSON.stringify(message.error)}`);
    }
  }

  proc.stdout.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const separator = buffer.indexOf("\r\n\r\n");
      if (separator < 0) break;
      const header = buffer.slice(0, separator).toString("ascii");
      const match = /Content-Length:\s*(\d+)/i.exec(header);
      const start = separator + 4;
      if (!match) {
        buffer = buffer.slice(start);
        continue;
      }
      const length = Number(match[1]);
      if (buffer.length < start + length) break;
      const body = buffer.slice(start, start + length).toString("utf8");
      buffer = buffer.slice(start + length);
      try {
        handle(JSON.parse(body));
      } catch (err) {
        onLog(`ada-lsp: 无法解析消息: ${err}`);
      }
    }
  });

  proc.stderr.on("data", (chunk) => onLog(chunk.toString().trim()));
  proc.on("exit", (code) => {
    if (code !== 0 && code !== null) onLog(`ada-lsp 退出，代码 ${code}`);
  });

  function send(message) {
    if (disposed || !proc.stdin.writable) return;
    proc.stdin.write(encode(message));
  }

  return {
    proc,
    /** initialize + initialized 握手 */
    initialize(params = {}) {
      send({ jsonrpc: "2.0", id: nextId++, method: "initialize", params });
      send({ jsonrpc: "2.0", method: "initialized", params: {} });
    },
    didOpen({ uri, text, languageId = "ada", version = 1 }) {
      send({
        jsonrpc: "2.0",
        method: "textDocument/didOpen",
        params: { textDocument: { uri, languageId, version, text } },
      });
    },
    didChange({ uri, text, version }) {
      send({
        jsonrpc: "2.0",
        method: "textDocument/didChange",
        params: {
          textDocument: { uri, version },
          contentChanges: [{ text }],
        },
      });
    },
    didClose(uri) {
      send({
        jsonrpc: "2.0",
        method: "textDocument/didClose",
        params: { textDocument: { uri } },
      });
    },
    /** 等待某个文档的下一次诊断（编辑器之外做断言时很方便） */
    waitForDiagnostics(uri, timeoutMs = 10000) {
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          waiters.delete(uri);
          reject(new Error(`等待 ${uri} 的诊断超时`));
        }, timeoutMs);
        waiters.set(uri, { resolve, timer });
      });
    },
    dispose() {
      if (disposed) return;
      send({ jsonrpc: "2.0", id: nextId++, method: "shutdown" });
      send({ jsonrpc: "2.0", method: "exit" });
      disposed = true;
      for (const waiter of waiters.values()) clearTimeout(waiter.timer);
      waiters.clear();
      setTimeout(() => {
        if (!proc.killed) proc.kill();
      }, 2000).unref();
    },
  };
}

module.exports = { createClient, encode };
