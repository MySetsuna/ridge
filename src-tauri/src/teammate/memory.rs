//! Workspace Memory sidecar 的**唯一** doc 级读改写（M1，iteration 14）。
//!
//! 文件：`{app_data}/workspace-memory/{wid}.json`；节：`suspendedPanes`（切片一，
//! suspend.rs 经此写）、`decisions`（切片二，hitl.rs 三消费点写，环形上限 50）。
//! 进程互斥 + 原子写（temp+rename）；**IO 全程 fail-open**（失败 warn 不阻断）；
//! 文件删除条件 = doc 空（无任何节）。`DIR` 于 lib.rs setup 注入一次；未注入
//! （纯单测）时全局入口 no-op，dir 注入版供测试。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

static DIR: OnceLock<PathBuf> = OnceLock::new();
/// doc 级读改写互斥（单进程单写者；跨节并发写防丢节）。
static FILE_MUTEX: Mutex<()> = Mutex::new(());

/// lib.rs setup 注入一次（后续调用忽略）。
pub fn init_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

pub fn dir() -> Option<&'static Path> {
    DIR.get().map(PathBuf::as_path)
}

fn path_of(dir: &Path, wid: Uuid) -> PathBuf {
    dir.join(format!("{wid}.json"))
}

/// doc 级读改写：读现 doc（无/损坏 → 空对象）→ `f` 就地改 → 空 doc 删文件，
/// 否则原子写。互斥内完成。
pub fn update(dir: &Path, wid: Uuid, f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) {
    let _guard = FILE_MUTEX.lock();
    let path = path_of(dir, wid);
    let mut doc = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    // updatedAt 为元数据：判空/重写前剥离，写出时（非空 doc）自动补。
    doc.remove("updatedAt");
    f(&mut doc);
    if !doc.is_empty() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        doc.insert("updatedAt".to_string(), serde_json::json!(now_ms));
    }
    let write = || -> std::io::Result<()> {
        if doc.is_empty() {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{wid}.json.tmp"));
        std::fs::write(&tmp, serde_json::Value::Object(doc).to_string())?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    };
    if let Err(e) = write() {
        tracing::warn!(target: "ridge::memory", error = %e, %wid, "sidecar 写入失败（fail-open）");
    }
}

/// 读整 doc（无/损坏 → None）。
pub fn read(dir: &Path, wid: Uuid) -> Option<serde_json::Value> {
    let s = std::fs::read_to_string(path_of(dir, wid)).ok()?;
    serde_json::from_str(&s).ok()
}

/// 删整文件（工作区关闭）。
pub fn remove(dir: &Path, wid: Uuid) {
    let _guard = FILE_MUTEX.lock();
    let _ = std::fs::remove_file(path_of(dir, wid));
}

/// decisions 环形上限（切片二）。
pub const DECISIONS_CAP: usize = 50;

/// 追加一条裁决记录（尾插，超限去头）。**调用方保证条目不含命令全文。**
pub fn append_decision(dir: &Path, wid: Uuid, entry: serde_json::Value) {
    update(dir, wid, |doc| {
        let arr = doc
            .entry("decisions".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let Some(list) = arr.as_array_mut() {
            list.push(entry);
            let overflow = list.len().saturating_sub(DECISIONS_CAP);
            if overflow > 0 {
                list.drain(0..overflow);
            }
        }
    });
}

/// 全局便捷入口：DIR 未注入即 no-op（fail-open，纯单测态）。
pub fn append_decision_global(wid: Uuid, entry: serde_json::Value) {
    if let Some(d) = dir() {
        append_decision(d, wid, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_preserves_sections_caps_decisions_and_deletes_empty() {
        let dir = std::env::temp_dir().join(format!("ridge-memory-test-{}", Uuid::new_v4()));
        let wid = Uuid::new_v4();

        // 两节共存：写 suspendedPanes 后追加 decision，RMW 互不丢节。
        update(&dir, wid, |doc| {
            doc.insert("suspendedPanes".into(), serde_json::json!(["p1"]));
        });
        append_decision(&dir, wid, serde_json::json!({ "verdict": "reject", "n": 0 }));
        let doc = read(&dir, wid).expect("doc exists");
        assert_eq!(doc["suspendedPanes"][0], "p1");
        assert_eq!(doc["decisions"].as_array().unwrap().len(), 1);

        // cap：塞满溢出去头留尾。
        for n in 1..=(DECISIONS_CAP + 5) {
            append_decision(&dir, wid, serde_json::json!({ "verdict": "approve", "n": n }));
        }
        let doc = read(&dir, wid).unwrap();
        let list = doc["decisions"].as_array().unwrap();
        assert_eq!(list.len(), DECISIONS_CAP);
        assert_eq!(list.last().unwrap()["n"], DECISIONS_CAP + 5);

        // 清空两节 → 文件删除。
        update(&dir, wid, |doc| {
            doc.clear();
        });
        assert!(read(&dir, wid).is_none());
        assert!(!path_of(&dir, wid).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
