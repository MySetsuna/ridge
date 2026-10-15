// Retired native frontend owner; Runtime now owns the Cloud controller.
async function openPublicHost(kind: 'remote' | 'rdg', label: string, addr: string, token?: string): Promise<RegisteredHostLink> {
  const deviceName = addr.trim();
  const code = token?.trim();
  if (!deviceName || !code) throw new Error('公网设备名或 TOTP 无效');
  return {
    hostId: `cloud:${deviceName}`, kind, label: label.trim() || deviceName,
    detail: '公网同账号 · Cloud E2EE', deviceName,
    link: await connectCloudHostTopologyLink(deviceName, code),
  };
}
