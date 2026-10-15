export async function markHostPaneAgent(
  hostId: string,
  workspaceId: string,
  paneId: string,
  on: boolean,
): Promise<void> {
  const link = topologyLink(hostId);
  const mark = link.markPaneAgent;
  if (!mark) throw new Error('该主机不支持 Agent 标记');
  await mark.call(link, workspaceId, paneId, on);
  await refreshLinkedHostAfterMutation(hostId);
}
