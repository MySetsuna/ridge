---
name: windows-network-config
description: Windows网络配置技能，包括IP设置、DNS配置、网络故障排查
---
# Windows网络配置技能

## 基本网络命令
- `ipconfig` - 查看IP配置信息
- `ipconfig /all` - 查看详细网络信息
- `ipconfig /release` - 释放当前IP地址
- `ipconfig /renew` - 重新获取IP地址
- `ping <目标IP或域名>` - 测试网络连通性
- `tracert <目标域名>` - 跟踪路由路径

## 网络配置方法
### 1. 命令行配置静态IP
```cmd
netsh interface ip set address "本地连接" static 192.168.1.100 255.255.255.0 192.168.1.1
netsh interface ip set dns "本地连接" static 8.8.8.8
```

### 2. 查看网络连接状态
```cmd
netstat -an
netstat -rn
```

## 故障排查步骤
1. 检查物理连接（网线、WiFi）
2. 运行 `ipconfig /all` 检查IP配置
3. 使用 `ping` 测试基本连通性
4. 检查防火墙设置
5. 重启网络服务：`netsh winsock reset`

## 常见问题
- **无法连接网络**：检查IP配置、DNS设置
- **网速慢**：检查带宽占用、后台程序
- **特定网站无法访问**：检查DNS、hosts文件