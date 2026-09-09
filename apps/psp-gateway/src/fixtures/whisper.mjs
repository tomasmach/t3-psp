#!/usr/bin/env node
import * as NodeFS from "node:fs";

const args = process.argv.slice(2);
const input = args[args.indexOf("-f") + 1];
const output = args[args.indexOf("-of") + 1];
const file = NodeFS.openSync(input, "r");
try {
  const header = Buffer.alloc(46);
  NodeFS.readSync(file, header, 0, header.length, 0);
  const last = Buffer.alloc(2);
  NodeFS.readSync(file, last, 0, 2, NodeFS.fstatSync(file).size - 2);
  NodeFS.writeFileSync(
    `${output}.txt`,
    `${header.readUInt32LE(40) / 2}:${header.readInt16LE(44)}:${last.readInt16LE(0)}\n`,
  );
} finally {
  NodeFS.closeSync(file);
}
