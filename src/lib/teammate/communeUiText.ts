export const communeUiText = {
  zh: {
    taskPrepared: '受理尚未确认', taskSubmitted: '已提交', taskQueued: '排队', taskDispatching: '派发中', taskRunning: '执行中',
    taskWaitingPermission: '待审批', taskReplyEnded: '回复结束；目标待审', taskCancelled: '已取消', taskCancelledBeforeDispatch: '派发前取消',
    taskRejected: '拒收', taskDeliveryFailed: '投递失败，未确认执行', taskUnknown: '结果未知', taskExpired: '已过期', taskReportedCompleted: '成员报告完成；待审', taskReportedFailed: '成员报告失败',
    reviewDispatch: '审定成员与目标，再派发', action: '协作动作', broadcast: '向选定成员广播', assign: '审定成员分工', selectMember: '选择 {name}', online: '在线', offline: '离线',
    noBinding: '无可用绑定/权限', memberTask: '{name} 的审定任务', memberObjective: '此成员的任务与交付要求', broadcastTarget: '广播目标', broadcastPlaceholder: '发给上述已勾选成员的同一目标',
    staleSelection: '编组或会话绑定已变，须重新审定。', clearSelection: '清除旧选择', dispatch: '确认派发（{count}）', retryDispatch: '重试原派发确认',
    groupRevision: '组修订 {revision}', groupTask: '组任务', objectiveExcerpt: '…（目标摘录，全文保留于任务账本）', fullObjective: '查看完整目标', previous: '前文', next: '后文',
    session: '会话', binding: '绑定', task: '任务', protocolArtifact: '协议产物', unverified: '未核验', rawProtocol: '查看原始协议记录',
    recentArtifacts: '仅列最近 50 条协议产物；原事件保留于历史。', queryCancel: '重新查询并请求取消', cancelGroupTask: '请求取消此组任务',
    cancellationPending: '已请求取消；以各成员协议终态为准。', individualConfirm: '各成员逐项确认；回复结束不作目标完成。',
    recentDispatches: '当前工作区最近 10 次组派发；旧记录仍留任务账本。',
    operationFailed: '操作失败：{error}',
    approvalInvalid: '审批已失效或已由其他客户端处理；请核对原任务状态，勿重复提交。', approvalUnknown: '审批结果未确认：{error}。请核对原任务状态，勿重复提交任务。',
    approvalExpired: '审批已失效或已由其他客户端处理。', approvalUnknownBrief: '审批结果未确认：{error}；请核对原任务状态。', cancelUnknown: '取消尚未确认：{error}',
    rejectionExpired: '该拒绝事件已失效；外部操作未被 Ridge 重试。', rejectionUnknown: '关闭报告结果未确认：{error}。正在重新读取报告列表。',
    rejectedExternally: '外部执行被拒', authorize: '需要你的授权', from: '来自 {name}', executor: '执行者', policySource: '策略来源', unknown: '未识别', request: '请求', nextStep: '下一步', editCommand: '修改命令',
    reject: '拒绝', cancelGroup: '取消所属组任务', cancelTask: '取消此任务', acknowledge: '知悉', modifyAndRun: '改后执行', modify: '修改', approveContinue: '批准并继续原操作',
  },
  en: {
    taskPrepared: 'Acceptance unconfirmed', taskSubmitted: 'Submitted', taskQueued: 'Queued', taskDispatching: 'Dispatching', taskRunning: 'Running',
    taskWaitingPermission: 'Awaiting approval', taskReplyEnded: 'Reply ended; objective needs review', taskCancelled: 'Cancelled', taskCancelledBeforeDispatch: 'Cancelled before dispatch',
    taskRejected: 'Rejected', taskDeliveryFailed: 'Delivery failed; execution unconfirmed', taskUnknown: 'Outcome unknown', taskExpired: 'Expired', taskReportedCompleted: 'Member reported completion; review needed', taskReportedFailed: 'Member reported failure',
    reviewDispatch: 'Review members and objectives before dispatch', action: 'Collaboration action', broadcast: 'Broadcast to selected members', assign: 'Assign reviewed objectives', selectMember: 'Select {name}', online: 'Online', offline: 'Offline',
    noBinding: 'No usable binding or permission', memberTask: '{name} objective', memberObjective: 'Objective and deliverables for this member', broadcastTarget: 'Broadcast objective', broadcastPlaceholder: 'Same objective for the selected members above',
    staleSelection: 'Group or session binding changed. Review the selection again.', clearSelection: 'Clear old selection', dispatch: 'Confirm dispatch ({count})', retryDispatch: 'Retry confirmation for the original dispatch',
    groupRevision: 'Group revision {revision}', groupTask: 'Group task', objectiveExcerpt: '… (objective excerpt; full text remains in the task ledger)', fullObjective: 'View full objective', previous: 'Earlier', next: 'Later',
    session: 'Session', binding: 'Binding', task: 'Task', protocolArtifact: 'Protocol artifact', unverified: 'Unverified', rawProtocol: 'View raw protocol event',
    recentArtifacts: 'Showing the latest 50 protocol artifacts; original events remain in history.', queryCancel: 'Check again and request cancellation', cancelGroupTask: 'Request cancellation for this group task',
    cancellationPending: 'Cancellation requested; each member protocol determines the final state.', individualConfirm: 'Each member is confirmed separately; a reply ending does not mean the objective is complete.',
    recentDispatches: 'Latest 10 group dispatches in this workspace; older records remain in the task ledger.',
    operationFailed: 'Operation failed: {error}',
    approvalInvalid: 'Approval expired or another client handled it. Check the original task; do not submit it again.', approvalUnknown: 'Approval result unconfirmed: {error}. Check the original task; do not resubmit it.',
    approvalExpired: 'Approval expired or another client handled it.', approvalUnknownBrief: 'Approval result unconfirmed: {error}. Check the original task.', cancelUnknown: 'Cancellation unconfirmed: {error}',
    rejectionExpired: 'This rejection event expired; Ridge did not retry the external action.', rejectionUnknown: 'Dismissal result unconfirmed: {error}. Reloading the report list.',
    rejectedExternally: 'External execution rejected', authorize: 'Authorization required', from: 'From {name}', executor: 'Executor', policySource: 'Policy source', unknown: 'Unknown', request: 'Request', nextStep: 'Next step', editCommand: 'Edit command',
    reject: 'Reject', cancelGroup: 'Cancel group task', cancelTask: 'Cancel this task', acknowledge: 'Acknowledge', modifyAndRun: 'Modify and run', modify: 'Modify', approveContinue: 'Approve and continue original action',
  },
} as const;

export type CommuneUiTextKey = keyof typeof communeUiText.zh;
export function communeText(locale: string, key: CommuneUiTextKey, vars: Record<string, string | number> = {}): string {
  const template = communeUiText[locale === 'en' ? 'en' : 'zh'][key];
  return template.replace(/\{(\w+)\}/g, (_, name: string) => String(vars[name] ?? `{${name}}`));
}
