import { createWorkspaceShare } from '@ridge/remote/shared/cloud/apiClient';
import { snapshot as authSnapshot } from '@ridge/remote/shared/cloud/auth';
import { alertDialog, promptDialog } from '$lib/components/RidgeDialog.svelte';

export async function shareWorkspaceWithAccount(input: {
  workspaceId: string;
  workspaceName?: string;
  deviceName?: string;
}): Promise<boolean> {
  const auth = authSnapshot();
  const deviceName = input.deviceName || auth.deviceName;
  if (!auth.userToken || !deviceName) {
    await alertDialog({
      title: '无法分享',
      message: '请先登录 Ridge Cloud，并激活对应设备。',
    });
    return false;
  }
  const grantee = await promptDialog({
    title: '分享工作区',
    message: `将「${input.workspaceName?.trim() || '当前工作区'}」分享给另一账户。当前版本授予操作权限；不可二次转发主机或 Remote。`,
    placeholder: '对方用户名或邮箱',
  });
  if (!grantee?.trim()) return false;
  try {
    await createWorkspaceShare(auth.userToken, {
      deviceName,
      workspaceId: input.workspaceId,
      grantee: grantee.trim(),
      role: 'operator',
    });
    await alertDialog({
      title: '邀请已发送',
      message: '对方接受后，可在「接入」面板打开此工作区。',
    });
    return true;
  } catch (error) {
    await alertDialog({
      title: '分享失败',
      message: error instanceof Error ? error.message : String(error),
      danger: true,
    });
    return false;
  }
}
