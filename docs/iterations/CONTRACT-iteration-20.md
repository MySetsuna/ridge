# CONTRACT — Iteration 20（Explorer free-follow body resize）

## 目标

资源管理器文件树与下方展示域拖拽：无拘无束鼠标跟随；下方区域随拖实时压缩。

## 验收

1. `computeBodyHeightFromDrag` / `clampBodyHeight` 单测绿（含越过原 lower-header Y）。  
2. Explorer 产品路径调用上述函数；`.explorer-lower` 可压缩；拖中 pointer 隔离。  
3. NLM 本弧 open=0；不碰并行会话愿景。

## 停机

闸红。
