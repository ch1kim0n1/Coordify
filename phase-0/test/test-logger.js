'use strict';

const assert = require('assert');
const fs = require('fs');
const path = require('path');

// Clean slate for test
const PAYLOADS_DIR = path.resolve(__dirname, '..', 'results', 'payloads');
const LATENCY_FILE = path.resolve(__dirname, '..', 'results', 'latency.jsonl');

// Remove old latency file for clean test
if (fs.existsSync(LATENCY_FILE)) fs.unlinkSync(LATENCY_FILE);

const logger = require('../hooks/logger');
const startedAt = Date.now() - 42; // simulate 42ms elapsed

// Test 1: capture writes a payload file
logger.capture('TestHook', { foo: 'bar', nested: { x: 1 } });

const files = fs.readdirSync(PAYLOADS_DIR).filter(f => f.startsWith('TestHook-'));
assert.ok(files.length > 0, 'capture() must write at least one payload file');

const written = JSON.parse(fs.readFileSync(path.join(PAYLOADS_DIR, files[files.length - 1]), 'utf8'));
assert.strictEqual(written.hookName, 'TestHook', 'hookName must match');
assert.strictEqual(written.payload.foo, 'bar', 'payload must be preserved');
assert.strictEqual(written.payload.nested.x, 1, 'nested payload must be preserved');
assert.ok(written.capturedAt, 'capturedAt must be set');

// Test 2: finish writes a latency record
logger.finish('TestHook', startedAt);

const lines = fs.readFileSync(LATENCY_FILE, 'utf8').trim().split('\n').filter(Boolean);
assert.ok(lines.length > 0, 'finish() must write at least one latency record');

const latency = JSON.parse(lines[lines.length - 1]);
assert.strictEqual(latency.hookName, 'TestHook', 'latency hookName must match');
assert.ok(latency.durationMs >= 42, 'durationMs must be at least the simulated elapsed time');

// Test 3: capture does not throw on bad payload
assert.doesNotThrow(() => logger.capture('BadHook', undefined));
assert.doesNotThrow(() => logger.capture('BadHook', null));

console.log('test-logger.js: all assertions passed');
