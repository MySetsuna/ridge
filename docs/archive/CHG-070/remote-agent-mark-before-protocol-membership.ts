// Archived LAN adapter method. Protocol membership is managed by native-trusted Runtime clients.
async markPaneAgent(
  workspaceId: string,
  paneId: string,
  on: boolean,
  agentId?: string,
): Promise<void> {
  const data = (await this._sendAndWait(
    {
      type: 'invoke-request',
      cmd: on ? 'register_teammate_agent' : 'release_teammate_agent',
      args: on
        ? { workspaceId, paneId, agentId: agentId || 'agent' }
        : { workspaceId, paneId },
      _reqId: ++this._reqCounter,
    },
    'invoke-result',
  )) as { _error?: unknown };
  if (data._error) throw new Error(unknownText(data._error, 'remote error'));
}

// Archived Cloud adapter method.
async markPaneAgent(
  workspaceId: string,
  paneId: string,
  on: boolean,
  agentId?: string,
): Promise<void> {
  if (on) {
    await this._invokePane({ workspaceId, paneId }, 'register_teammate_agent', {
      workspaceId,
      paneId,
      agentId: agentId || 'agent',
    });
  } else {
    await this._invokePane(
      { workspaceId, paneId },
      'release_teammate_agent',
      { workspaceId, paneId },
    );
  }
  void this._refreshPanes();
}
