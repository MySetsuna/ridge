import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { join } from 'node:path';

export async function verifyRuntimeHostReconnectUi({ page, root, command }) {
  const rejection = createServer((_request, response) => {
    response.writeHead(401, { 'content-type': 'application/json' });
    response.end('{}');
  });
  rejection.listen(0, '127.0.0.1'); await once(rejection, 'listening');
  let hostId;
  const label = 'Owned reconnect rejection';
  try {
    hostId = await command('connect_host', { kind: 'remote', label,
      addr: `http://127.0.0.1:${rejection.address().port}`, token: 'OWNED_INVALID_FIXTURE_CODE' });
    assert.equal((await command('host_list_snapshot')).find(host => host.id === hostId)?.status, 'error');
    await page.evaluate(async () => {
      const [{ mount, unmount }, { default: HostsPanel }, { bridge }] = await Promise.all([
        import('/node_modules/svelte/src/index-client.js'),
        import('/src/lib/components/hosts/HostsPanel.svelte'),
        import('/src/lib/transport/tauriShim/bridge.ts'),
      ]);
      const target = document.createElement('div');
      target.dataset.ownedHostReconnect = 'true';
      Object.assign(target.style, { position: 'fixed', inset: '0', zIndex: '100', background: '#171717', overflow: 'auto' });
      document.body.appendChild(target);
      const original = bridge.invoke;
      const methods = [];
      bridge.invoke = function(method, ...args) {
        if (methods.length < 256) methods.push(method);
        return original.call(this, method, ...args);
      };
      const instance = mount(HostsPanel, { target });
      window.__ownedHostReconnectUi = { methods, async close() {
        await unmount(instance); bridge.invoke = original; target.remove();
      } };
    });
    const panel = page.locator('[data-owned-host-reconnect="true"]');
    await panel.getByText(label, { exact: true }).waitFor({ timeout: 30000 });
    await page.waitForTimeout(11000); // Cover two production five-second poll intervals.
    await page.waitForFunction(() => window.__ownedHostReconnectUi.methods.filter(method => method === 'host_list_snapshot').length >= 3, undefined, { timeout: 20000 });
    await page.screenshot({ path: join(root, 'runtime-host-reconnect.png') });
    await panel.locator('button[title="重新认证连接"]').click();
    const dialog = page.getByRole('dialog', { name: '连接远端主机' });
    await dialog.waitFor(); await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await panel.locator('button[title="取消重连"]').click();
    const deadline = Date.now() + 10000;
    while ((await command('host_list_snapshot')).find(host => host.id === hostId)?.status !== 'disconnected') {
      assert.ok(Date.now() < deadline, 'UI cancel did not commit Runtime disconnect');
      await new Promise(done => setTimeout(done, 50));
    }
    const methods = await page.evaluate(() => [...window.__ownedHostReconnectUi.methods]);
    assert.ok(methods.includes('disconnect_host'), 'cancel must use the shared Runtime command');
    for (const retired of ['step_host_reconnect', 'cancel_host_reconnect', 'pump_host_output', 'get_outbound_stats', 'get_foreign_history_tail']) {
      assert.ok(!methods.includes(retired), `native Hosts panel invoked retired owner: ${retired}`);
    }
    return { checks: ['native error Host remains visible without a transport projection',
      'three real Hosts polls do not drive retired business owners',
      'retry opens authentication dialog; cancel commits shared Runtime disconnect'], methods,
      screenshot: join(root, 'runtime-host-reconnect.png') };
  } finally {
    await page.evaluate(() => window.__ownedHostReconnectUi?.close()).catch(() => {});
    if (hostId) await command('forget_host', { hostId });
    rejection.closeAllConnections(); await new Promise(done => rejection.close(done));
  }
}
