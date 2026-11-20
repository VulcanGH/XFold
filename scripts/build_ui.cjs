// Run with Node and TypeScript installed, or pass the path to typescript.js.
const ts = require(process.argv[2] || "typescript");
const path = require("node:path");
const root = path.resolve(__dirname, "..");
const source = path.join(root, "ui", "app.ts");
const program = ts.createProgram([source], {
    strict: true,
    noEmitOnError: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ES2022,
    outDir: path.join(root, "ui"),
    newLine: ts.NewLineKind.LineFeed,
});
const before = ts.getPreEmitDiagnostics(program);
if (before.length) {
    console.error(ts.formatDiagnosticsWithColorAndContext(before, {
        getCurrentDirectory: () => root,
        getCanonicalFileName: (file) => file,
        getNewLine: () => "\n",
    }));
    process.exitCode = 1;
} else {
    const result = program.emit();
    if (result.emitSkipped) process.exitCode = 1;
}
