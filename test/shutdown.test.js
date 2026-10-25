const { test } = require('node:test');
const assert = require('node:assert');
const path = require('path');
const { spawn } = require('child_process');

test('SIGTERM stops the addon with exit code 0', async () => {
  const child = spawn('node', [path.join(__dirname, '..', 'index.js')], {
    env: { ...process.env, PORT: '0', NO_CACHE: 'true', FANART_APIKEY: 'test', MONGODB_URI: '' },
    stdio: ['ignore', 'pipe', 'inherit']
  });
  const exited = new Promise((resolve) => child.once('exit', (code, signal) => resolve({ code, signal })));
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('addon did not start')), 10000);
    child.stdout.on('data', (chunk) => {
      if (chunk.toString().includes('Started addon')) {
        clearTimeout(timer);
        resolve();
      }
    });
  });
  child.kill('SIGTERM');
  let timer;
  const result = await Promise.race([
    exited,
    new Promise((resolve) => { timer = setTimeout(() => resolve({ timeout: true }), 5000); })
  ]);
  clearTimeout(timer);
  if (result.timeout) {
    child.kill('SIGKILL');
  }
  assert.deepStrictEqual(result, { code: 0, signal: null });
});
