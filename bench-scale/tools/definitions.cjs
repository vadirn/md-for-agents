// Print every candidate definition in a TypeScript or JavaScript corpus as
// JSON lines, with spans from the TypeScript compiler rather than the outline
// crate, so the tool under test never grades itself.
//
// Usage: node definitions.cjs <typescript.js> <corpus root> < file-list
//
// A candidate is a function, method, class, class field holding a function,
// or top-level const holding a function. Its span runs from its first token,
// after any doc comment, to its last line.

const fs = require("fs");
const path = require("path");
const ts = require(process.argv[2]);
const root = process.argv[3];

function lineOf(sf, pos) {
  return sf.getLineAndCharacterOfPosition(pos).line + 1;
}

function isFunction(node) {
  return node && (ts.isArrowFunction(node) || ts.isFunctionExpression(node));
}

function emit(sf, file, node, name, kind) {
  const start = lineOf(sf, node.getStart(sf));
  const end = lineOf(sf, node.getEnd());
  console.log(JSON.stringify({ path: file, name, kind, start, end }));
}

function members(sf, file, cls) {
  for (const m of cls.members) {
    const name = m.name && m.name.getText(sf);
    if ((ts.isMethodDeclaration(m) || ts.isGetAccessor(m) || ts.isSetAccessor(m)) && m.body) {
      emit(sf, file, m, name, "method");
    } else if (ts.isConstructorDeclaration(m) && m.body) {
      emit(sf, file, m, "constructor", "method");
    } else if (ts.isPropertyDeclaration(m) && isFunction(m.initializer)) {
      emit(sf, file, m, name, "method");
    }
  }
}

for (const file of fs.readFileSync(0, "utf8").split("\n").filter(Boolean)) {
  const text = fs.readFileSync(path.join(root, file), "utf8");
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  for (const stmt of sf.statements) {
    if (ts.isFunctionDeclaration(stmt) && stmt.body && stmt.name) {
      emit(sf, file, stmt, stmt.name.text, "function");
    } else if (ts.isClassDeclaration(stmt) && stmt.name) {
      emit(sf, file, stmt, stmt.name.text, "class");
      members(sf, file, stmt);
    } else if (ts.isVariableStatement(stmt)) {
      const decls = stmt.declarationList.declarations;
      if (decls.length === 1 && isFunction(decls[0].initializer)) {
        emit(sf, file, stmt, decls[0].name.getText(sf), "function");
      }
    }
  }
}
