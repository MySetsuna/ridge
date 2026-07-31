---
name: windows-security-settings
description: Windows安全设置技能，包括防���墙配置、用户账户管理、安全策略设置
---
# Windows安全设置技能

## 防火墙配置
- `wf.msc` - Windows防火墙高级安全
- `netsh advfirewall firewall add rule name="规则名" dir=in action=allow` - 添加入站规则
- `netsh advfirewall firewall delete rule name="规则名"` - 删除防火墙规则
- `netsh advfirewall set allprofiles state on` - 启用所有防火墙配置文件

## 用户账户管理
- `lusrmgr.msc` - 本地用户和组管理
- `net user <用户名>` - 查看用户信息
- `net user <用户名> <密码>` - 设置用户密码
- `net localgroup administrators <用户名> /add` - 添加用户到管理员组

## 安全策略设置
- `secpol.msc` - 本地安全策略
- `gpedit.msc` - 组策略编辑器
- `auditpol /get /category:*` - 查看审计策略
- `auditpol /set /category:"帐户登录" /success:enable /failure:enable` - 启用登录审计

## 安全基线检查
```cmd
@echo off
echo 检查系统安全设置...
echo 1. 检查密码策略
net accounts
echo 2. 检查用户权限
whoami /priv
echo 3. 检查系统更新
wmic os get lastbootuptime
echo 4. 检查防火墙状态
netsh advfirewall show allprofiles
pause
```

## 常见安全配置
1. **密码策略**：要求复杂密码，定期更换
2. **账户锁定**：设置失败登录次数限制
3. **权限最小化**：普通用户使用标准账户
4. **自动更新**：启用Windows自动更新
5. **防病毒**：安装并更新杀毒软件

## 安全注意事项
- 定期更改密码
- 使用强密码（大小写字母+数字+特殊字符）
- 关闭不必要的端口和服务
- 定期备份重要数据
- 监控系统日志