---
name: windows-system-maintenance
description: Windows系统维护技能，包括磁盘清理、系统优化、性能监控
---
# Windows系统维护技能

## 磁盘管理
- `cleanmgr` - 打开磁盘清理工具
- `defrag <盘符>` - 磁盘碎片整理
- `chkdsk <盘符> /f` - 检查并修复磁盘错误
- `sfc /scannow` - 系统文件检查器

## 系统优化命令
- `msconfig` - 系统配置实用程序
- `taskmgr` - 任务管理器
- `perfmon` - 性能监视器
- `msinfo32` - 系统信息

## 服务管理
- `services.msc` - 服务管理控制台
- `net start <服务名>` - 启动服务
- `net stop <服务名>` - 停止服务
- `sc query <服务名>` - 查询服务状态

## 性能监控
- `wmic cpu get loadpercentage` - CPU使用率
- `wmic os get totalvisiblememorysize,freephysicalmemory` - 内存使用情况
- `typeperf "\Processor(_Total)\% Processor Time" -sc 1` - CPU实时监控

## 常用维护脚本
```cmd
@echo off
echo 正在清理系统...
cleanmgr /sagerun:1
echo 正在检查系统文件...
sfc /scannow
echo 正在优化启动项...
msconfig /startup
pause
```

## 维护建议
- 定期清理临时文件
- 更新系统补丁
- 管理启动项
- 监控系统性能
- 备份重要数据