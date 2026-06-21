import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  // TextMate highlighting is regex-based and approximate because most FSL
  // keywords are contextual and may be valid identifiers in other positions.
  const command = process.env.FSLC_LSP_COMMAND || "fslc-lsp";
  const serverOptions: ServerOptions = {
    run: {
      command,
      args: [],
      transport: TransportKind.stdio,
      options: { cwd: context.extensionPath, env: process.env }
    },
    debug: {
      command,
      args: [],
      transport: TransportKind.stdio,
      options: { cwd: context.extensionPath, env: process.env }
    }
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "fsl" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.fsl")
    }
  };

  client = new LanguageClient("fsl", "FSL Language Server", serverOptions, clientOptions);
  context.subscriptions.push(client);
  client.start();
}

export function deactivate(): Promise<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
