// 用**真实的** VSCode 扩展客户端核心去驱动真实 `ada-lsp` 进程，打印观察到的诊断。
//
// 由 crates/ada_lsp/tests/node_client.rs 调用：
//   node node_client_driver.js <ada-lsp 路径> <lsp-client.js 路径>
//
// 输出：一行 JSON（便于 Rust 侧解析断言）。

const path = require("path");

const binary = process.argv[2];
const clientPath = process.argv[3];
if (!binary || !clientPath) {
  console.error("用法: node node_client_driver.js <ada-lsp 路径> <lsp-client.js 路径>");
  process.exit(2);
}

const { createClient } = require(path.resolve(clientPath));

async function main() {
  const binary = process.argv[2];
  if (!binary) {
    console.error("用法: node node_client_driver.js <ada-lsp 路径>");
    process.exit(2);
  }

  const observed = [];
  const client = createClient({
    command: binary,
    onDiagnostics: (uri, diagnostics) => {
      observed.push({ uri, diagnostics });
    },
  });

  client.initialize({ processId: process.pid, rootUri: null, capabilities: {} });

  const uri = "file:///driver/demo.ada";

  // 1) 语法错误
  const first = client.waitForDiagnostics(uri);
  client.didOpen({ uri, text: "let x = (1 +", version: 1 });
  const syntax = await first;

  // 2) 改成编译错误
  const second = client.waitForDiagnostics(uri);
  client.didChange({ uri, text: "let a = 1\nlet b = 2\na = 3\n", version: 2 });
  const compile = await second;

  // 3) 修好后诊断清空
  const third = client.waitForDiagnostics(uri);
  client.didChange({ uri, text: "mut a = 1\nlet b = 2\na = 3\na + b\n", version: 3 });
  const fixed = await third;

  client.dispose();

  process.stdout.write(
    JSON.stringify({
      syntax,
      compile,
      fixed,
      publishes: observed.length,
    })
  );
}

main().catch((err) => {
  console.error(`driver 失败: ${err}`);
  process.exit(1);
});
