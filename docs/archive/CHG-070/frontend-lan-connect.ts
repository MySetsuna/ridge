  const link = new RemoteConnection();
  await waitForRemoteConnection(link, host, port, code, secure);
async function waitForRemoteConnection(
  link: RemoteConnection,
  host: string,
  port: number,
  code: string,
  secure: boolean,
): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      off();
      link.disconnect();
      reject(new Error(lanConnectionError(host, port, secure, 'LAN 主机连接超时', 'channel')));
    }, 10_000);
    const off = link.onStateChange((state) => {
      if (state === 'connected') {
        clearTimeout(timer);
        off();
        resolve();
      } else if (state === 'error') {
        clearTimeout(timer);
        off();
        const failure = link.lastFailure();
        reject(new Error(lanConnectionError(host, port, secure, failure?.message, failure?.category)));
      }
    });
    link.connect(host, port, code, 'code', secure);
  });
}

