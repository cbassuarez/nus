// Read-only oracle for a disposable benchmark file. No commands or UI timing.
const vscode=require('vscode'),fs=require('node:fs');
exports.activate=context=>{
 const check=()=>{
  const editor=vscode.window.activeTextEditor;
  if(!editor || editor.document.uri.fsPath!==process.env.NUS_BENCH_FILE)return;
  const text=editor.document.getText();
  if(text.length!==Number(process.env.NUS_BENCH_BYTES)||!text.startsWith('NUS benchmark fixture'))return;
  fs.writeFileSync(process.env.NUS_BENCH_READY,JSON.stringify({bytes:text.length,visible:true}));
 };
 context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(check));check();
};
