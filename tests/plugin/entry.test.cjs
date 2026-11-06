const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../../plugins/liteloader/main.cjs'), 'utf8');
function load(value, meta = {}) {
  const handlers = new Map(), opened = [], reads = [];
  vm.runInNewContext(source, {
    require(name) {
      if (name === 'electron') return { ipcMain: { handle: (key, fn) => handlers.set(key, fn) }, shell: { openExternal: async (uri) => opened.push(uri) } };
      if (name === 'node:path') return path.win32;
      if (name === 'node:fs/promises') return {
        lstat: async (location) => { reads.push(location); return { isFile: () => true, isSymbolicLink: () => false, size: 128, ...meta }; },
        readFile: async () => JSON.stringify(value),
      };
      throw new Error(`Unexpected capability: ${name}`);
    },
    process: { platform: 'win32', env: { LOCALAPPDATA: 'C:\\MOCK\\Local' } },
    LiteLoader: { versions: { liteloader: 'test' } },
  });
  return { handlers, opened, reads };
}
test('summary exposes counts and time only, from fixed manager data path', async () => {
  const app = load({ schema: 1, totalBytes: 42, files: 3, scannedAt: 123, recognized: true, root: 'SECRET', account: 'SECRET', media: 'SECRET' });
  const status = await app.handlers.get('qqnt-storage-manager:status')();
  assert.equal(JSON.stringify(status).includes('SECRET'), false);
  assert.deepEqual(JSON.parse(JSON.stringify(status.summary)), { totalBytes: 42, files: 3, scannedAt: 123, recognized: true });
  assert.equal(app.reads[0], 'C:\\MOCK\\Local\\io.github.qqnt-storage-manager\\public-summary.json');
  assert.equal(app.handlers.size, 2);
});
test('invalid, symbolic, oversized summaries are rejected', async () => {
  for (const app of [load({ schema: 1, totalBytes: -1 }), load({}, { isSymbolicLink: () => true }), load({}, { size: 65537 })]) {
    assert.equal((await app.handlers.get('qqnt-storage-manager:status')()).summary, null);
  }
});
test('open ignores renderer paths and arguments; preload exposes only two methods', async () => {
  const app = load({});
  await app.handlers.get('qqnt-storage-manager:open')({}, 'file:///secret', '--delete');
  assert.deepEqual(app.opened, ['qqnt-storage-manager://open']);
  let exposed;
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../../plugins/liteloader/preload.cjs'), 'utf8'), {
    require: () => ({ contextBridge: { exposeInMainWorld: (key, api) => { assert.equal(key, 'qqntStorageManager'); exposed = api; } }, ipcRenderer: { invoke: async () => true } }),
  });
  assert.deepEqual(Object.keys(exposed), ['getStatus', 'openManager']);
});
