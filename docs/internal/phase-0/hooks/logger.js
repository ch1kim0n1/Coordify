'use strict';

const fs = require('fs');
const path = require('path');

const RESULTS_DIR = path.resolve(__dirname, '..', 'results');
const PAYLOADS_DIR = path.join(RESULTS_DIR, 'payloads');
const LATENCY_FILE = path.join(RESULTS_DIR, 'latency.jsonl');

function ensureDirs() {
  fs.mkdirSync(PAYLOADS_DIR, { recursive: true });
}

function capture(hookName, payload) {
  try {
    ensureDirs();
    const ts = new Date().toISOString().replace(/[:.]/g, '-');
    const file = path.join(PAYLOADS_DIR, `${hookName}-${ts}.json`);
    const record = { hookName, capturedAt: new Date().toISOString(), payload };
    fs.writeFileSync(file, JSON.stringify(record, null, 2));
  } catch (_) {
    // never throw from hook context
  }
}

function finish(hookName, startedAt) {
  try {
    const durationMs = Date.now() - startedAt;
    const line = JSON.stringify({ hookName, startedAt: new Date(startedAt).toISOString(), durationMs }) + '\n';
    fs.appendFileSync(LATENCY_FILE, line);
  } catch (_) {
    // never throw from hook context
  }
}

module.exports = { capture, finish };
