const assert = require('node:assert/strict');
const fs = require('node:fs');
const test = require('node:test');
const { transpileTypeScript } = require('../scripts/typescript_source_tools.cjs');
require.extensions['.tsx'] = require.extensions['.ts'] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, 'utf8'), filename), filename);
const { formatInstanceAutostartJobLabel, formatInstanceAutostartJobDetail } = require('../src/instance-autostart-job.ts');
const { translate } = require('../src/i18n.tsx');
const catalogs = {
  'en-US': require('../src/i18n-messages.ts').EN_US_MESSAGES,
  'zh-CN': require('../src/i18n-messages-zh-cn.ts').ZH_CN_MESSAGES
};
const t = locale => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
const job = { id: 'startup', kind: 'StartInstance', target_id: 'server-a', label: 'Autostart Server A', status: 'Running' };
for (const [detail, chinese] of [
  ['Starting automatically with LGSM.', '正在随 LGSM 自动启动。'],
  ['Started automatically with LGSM.', '已随 LGSM 自动启动。'],
  ['Autostart skipped because the instance is already running or starting.', '实例已在运行或启动中，已跳过自动启动。'],
  ['Autostart cancelled.', '自动启动已取消。']
]) {
  test(`autostart job localizes ${detail}`, () => {
    assert.equal(formatInstanceAutostartJobDetail({ ...job, detail }, t('en-US')), detail);
    assert.equal(formatInstanceAutostartJobDetail({ ...job, detail }, t('zh-CN')), chinese);
    assert.equal(formatInstanceAutostartJobLabel(job, t('zh-CN')), '自动启动 Server A');
    assert.equal(formatInstanceAutostartJobLabel(job, t('en-US')), 'Autostart Server A');
  });
}
test('unrecognized diagnostics and unrelated job labels remain intact', () => {
  assert.equal(formatInstanceAutostartJobDetail({ ...job, detail: 'Missing executable: server.exe' }, t('zh-CN')), 'Missing executable: server.exe');
  assert.equal(formatInstanceAutostartJobLabel({ ...job, kind: 'InstallModule' }, t('zh-CN')), job.label);
});
