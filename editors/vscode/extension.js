// Ada-D VSCode 扩展：语法高亮 + 实时诊断（开发计划 T7.1 / T7.2）
//
// 语法高亮由 `syntaxes/ada.tmLanguage.json` 提供；
// 诊断由零依赖的 LSP 客户端 `lsp-client.js` 驱动 `ada-lsp` 子进程提供。
//
// 诊断内容与 `ada check` 完全一致（同一份静态检查代码路径），只做语法/编译错误，
// **不执行脚本**（编辑器里每次输入都会触发诊断，执行脚本会有副作用与卡顿风险）。

const vscode = require("vscode");
const { createClient } = require("./lsp-client");

let client = null;
let collection = null;

function serverCommand() {
  const configured = vscode.workspace.getConfiguration("ada").get("lsp.path");
  if (configured && configured.length > 0) {
    return { command: configured, args: [] };
  }
  return { command: "ada-lsp", args: [] };
}

function toVscodeDiagnostics(diagnostics) {
  return diagnostics.map((diagnostic) => {
    const range = new vscode.Range(
      diagnostic.range.start.line,
      diagnostic.range.start.character,
      diagnostic.range.end.line,
      diagnostic.range.end.character
    );
    const severity =
      diagnostic.severity === 1
        ? vscode.DiagnosticSeverity.Error
        : vscode.DiagnosticSeverity.Warning;
    const item = new vscode.Diagnostic(range, diagnostic.message, severity);
    item.source = diagnostic.source || "ada";
    return item;
  });
}

function activate(context) {
  collection = vscode.languages.createDiagnosticCollection("ada");
  context.subscriptions.push(collection);

  const { command, args } = serverCommand();
  client = createClient({
    command,
    args,
    onDiagnostics: (uri, diagnostics) => {
      collection.set(vscode.Uri.parse(uri), toVscodeDiagnostics(diagnostics));
    },
    onLog: (message) => console.error(`ada-lsp: ${message}`),
  });

  client.initialize({
    processId: process.pid,
    rootUri: vscode.workspace.workspaceFolders
      ? vscode.workspace.workspaceFolders[0].uri.toString()
      : null,
    capabilities: {},
  });

  const openAdaDocument = (document) => {
    client.didOpen({
      uri: document.uri.toString(),
      text: document.getText(),
      version: document.version,
    });
  };

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (document.languageId === "ada") openAdaDocument(document);
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      if (event.document.languageId !== "ada") return;
      client.didChange({
        uri: event.document.uri.toString(),
        text: event.document.getText(),
        version: event.document.version,
      });
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      collection.delete(document.uri);
      client.didClose(document.uri.toString());
    })
  );

  // 扩展激活时，对已经打开的 .ada 文件做一次诊断
  for (const document of vscode.workspace.textDocuments) {
    if (document.languageId === "ada") openAdaDocument(document);
  }
}

function deactivate() {
  if (client) {
    client.dispose();
    client = null;
  }
  if (collection) {
    collection.dispose();
    collection = null;
  }
}

module.exports = { activate, deactivate };
