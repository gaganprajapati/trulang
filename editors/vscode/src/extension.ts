import * as vscode from 'vscode';
import * as path from 'path';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration('trulang');
  const lspPath = config.get<string>('lsp.path', 'trulang');

  // Verify binary availability
  const serverExecutable: Executable = {
    command: lspPath,
    args: ['lsp'],
    options: {
      env: { ...process.env, RUST_LOG: 'info' }
    }
  };

  const serverOptions: ServerOptions = {
    run: serverExecutable,
    debug: serverExecutable
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'trulang' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.tr')
    },
    outputChannel: vscode.window.createOutputChannel('TruLang Language Server')
  };

  try {
    client = new LanguageClient(
      'trulangLsp',
      'TruLang Language Server',
      serverOptions,
      clientOptions
    );

    await client.start();
  } catch (err) {
    vscode.window.showErrorMessage(
      `Failed to start TruLang Language Server using command '${lspPath}'. Make sure 'trulang' is installed and in your PATH, or configure 'trulang.lsp.path' in Settings.`,
      'Install via Homebrew',
      'Open Documentation'
    ).then((selection) => {
      if (selection === 'Install via Homebrew') {
        const term = vscode.window.createTerminal('Install TruLang');
        term.show();
        term.sendText('brew tap gaganprajapati/trulang && brew install trulang');
      } else if (selection === 'Open Documentation') {
        vscode.env.openExternal(vscode.Uri.parse('https://gagn.dev/trulang/'));
      }
    });
  }

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand('trulang.restartServer', async () => {
      if (client) {
        await client.stop();
        await client.start();
        vscode.window.showInformationMessage('TruLang Language Server restarted.');
      }
    }),

    vscode.commands.registerCommand('trulang.runFile', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'trulang') {
        vscode.window.showWarningMessage('No active TruLang (.tr) file open.');
        return;
      }
      const filePath = editor.document.uri.fsPath;
      const term = vscode.window.activeTerminal || vscode.window.createTerminal('TruLang Run');
      term.show();
      term.sendText(`${lspPath} run "${filePath}"`);
    }),

    vscode.commands.registerCommand('trulang.checkFile', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'trulang') {
        vscode.window.showWarningMessage('No active TruLang (.tr) file open.');
        return;
      }
      const filePath = editor.document.uri.fsPath;
      const term = vscode.window.activeTerminal || vscode.window.createTerminal('TruLang Check');
      term.show();
      term.sendText(`${lspPath} check "${filePath}"`);
    }),

    vscode.commands.registerCommand('trulang.openDocs', () => {
      vscode.env.openExternal(vscode.Uri.parse('https://gagn.dev/trulang/'));
    }),

    vscode.commands.registerCommand('trulang.debugFile', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'trulang') {
        vscode.window.showWarningMessage('No active TruLang (.tr) file open.');
        return;
      }
      const filePath = editor.document.uri.fsPath;
      vscode.debug.startDebugging(undefined, {
        type: 'trulang',
        name: 'Debug TruLang File',
        request: 'launch',
        program: filePath,
        stopOnEntry: false
      });
    }),

    vscode.commands.registerCommand('trulang.profileFile', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'trulang') {
        vscode.window.showWarningMessage('No active TruLang (.tr) file open.');
        return;
      }
      const filePath = editor.document.uri.fsPath;
      const svgPath = filePath.replace(/\.tr$/, '.svg');
      const term = vscode.window.activeTerminal || vscode.window.createTerminal('TruLang Profile');
      term.show();
      term.sendText(`${lspPath} profile "${filePath}" -o "${svgPath}"`);
      vscode.window.showInformationMessage(`Visual memory flamegraph will be emitted to ${path.basename(svgPath)}`);
    }),

    vscode.debug.registerDebugAdapterDescriptorFactory('trulang', {
      createDebugAdapterDescriptor(_session: vscode.DebugSession): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
        return new vscode.DebugAdapterExecutable(lspPath, ['dap']);
      }
    })
  );
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}
