#!/usr/bin/env node
"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");

const bin = path.join(__dirname, "granttap-term");
const child = spawn(bin, ["mcp"], { stdio: "inherit" });

child.on("error", (error) => {
  process.stderr.write(`granttap-local: ${error.message}\n`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code == null ? 1 : code);
});
