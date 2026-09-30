//! 插件运行时：在 QuickJS 沙箱里执行插件入口脚本。
//!
//! 约定（与示例插件一致）：
//! - 脚本是普通的 JS，通过定义全局函数导出能力，例如 `function translate(input) {...}`；
//! - 出入参都走 JSON，宿主只认字符串与 JSON 可表达的结构；
//! - 宿主 API 挂在全局 `host` 上：`host.log(msg)`，申请了 clipboard 权限才有
//!   `host.clipboard.read()` / `host.clipboard.write(text)`。
//!
//! 沙箱边界：每次调用新建独立的 Runtime/Context，设有内存与栈上限；
//! 插件脚本由清单校验保证落在自己的目录内。**插件目前没有网络能力**，
//! 需要联网的扩展点要等后续把异步 HTTP 桥接进去，这里不做半成品。

use rquickjs::{Context, Ctx, Function, Object, Runtime, Value};
use std::path::Path;

/// 单个脚本的大小上限，防止误把大文件当脚本读进来
const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;
/// QuickJS 内存上限（字节）：插件跑飞了也不至于把应用拖垮
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// QuickJS 栈深度上限
const STACK_LIMIT: usize = 512 * 1024;

fn read_script(main: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(main).map_err(|e| format!("读取插件脚本失败: {e}"))?;
    if meta.len() > MAX_SCRIPT_BYTES {
        return Err(format!(
            "插件脚本过大（{} KB），上限 1 MB",
            meta.len() / 1024
        ));
    }
    std::fs::read_to_string(main).map_err(|e| format!("读取插件脚本失败: {e}"))
}

/// 安装宿主 API。权限没有申请到的能力不会出现在 `host` 上，
/// 插件调用时会得到 "not a function" 这类明确错误，而不是静默失效。
fn install_host<'js>(ctx: &Ctx<'js>, permissions: &[String]) -> Result<(), String> {
    let host = Object::new(ctx.clone()).map_err(|e| format!("创建 host 对象失败: {e}"))?;

    let log = Function::new(ctx.clone(), |msg: String| {
        crate::selection::log_line(&format!("plugin: {msg}"));
    })
    .map_err(|e| format!("注册 host.log 失败: {e}"))?;
    host.set("log", log).map_err(|e| e.to_string())?;

    if permissions.iter().any(|p| p == "clipboard") {
        let clipboard = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
        let read = Function::new(ctx.clone(), || -> String {
            arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .unwrap_or_default()
        })
        .map_err(|e| e.to_string())?;
        let write = Function::new(ctx.clone(), |text: String| -> bool {
            arboard::Clipboard::new()
                .and_then(|mut c| c.set_text(text))
                .is_ok()
        })
        .map_err(|e| e.to_string())?;
        clipboard.set("read", read).map_err(|e| e.to_string())?;
        clipboard.set("write", write).map_err(|e| e.to_string())?;
        host.set("clipboard", clipboard).map_err(|e| e.to_string())?;
    }

    ctx.globals()
        .set("host", host)
        .map_err(|e| format!("注入 host 失败: {e}"))
}

/// 调用插件导出的函数。`args_json` 与返回值都是 JSON 文本。
///
/// QuickJS 的句柄都不是 Send，所以整段跑在独立线程里，结果只把 String 带回来。
pub fn call(
    main: &Path,
    func: &str,
    args_json: &str,
    permissions: &[String],
) -> Result<String, String> {
    let script = read_script(main)?;
    let func = func.to_string();
    let args_json = args_json.to_string();
    let permissions: Vec<String> = permissions.to_vec();

    std::thread::spawn(move || -> Result<String, String> {
        let rt = Runtime::new().map_err(|e| format!("创建 JS 运行时失败: {e}"))?;
        rt.set_memory_limit(MEMORY_LIMIT);
        rt.set_max_stack_size(STACK_LIMIT);
        let ctx = Context::full(&rt).map_err(|e| format!("创建 JS 上下文失败: {e}"))?;

        ctx.with(|ctx| {
            install_host(&ctx, &permissions)?;

            ctx.eval::<(), _>(script.as_bytes())
                .map_err(|e| format!("插件脚本执行失败: {}", describe(&ctx, e)))?;

            let globals = ctx.globals();
            let f: Function = globals
                .get(func.as_str())
                .map_err(|_| format!("插件没有导出 {func}() 函数"))?;

            // 用 JSON.parse 把参数喂进去，避免手工拼 JS 字面量
            let literal = serde_json::to_string(&args_json).map_err(|e| e.to_string())?;
            let arg: Value = ctx
                .eval(format!("JSON.parse({literal})"))
                .map_err(|e| format!("构造参数失败: {}", describe(&ctx, e)))?;

            let out: Value = f
                .call((arg,))
                .map_err(|e| format!("{func}() 抛出异常: {}", describe(&ctx, e)))?;

            let stringify: Function = ctx
                .eval("(function (v) { return JSON.stringify(v === undefined ? null : v); })")
                .map_err(|e| format!("准备结果转换失败: {}", describe(&ctx, e)))?;
            stringify
                .call::<_, String>((out,))
                .map_err(|e| format!("转换返回值失败: {}", describe(&ctx, e)))
        })
    })
    .join()
    .map_err(|_| "插件线程异常退出".to_string())?
}

/// QuickJS 的异常带上下文栈，转成能读的字符串
fn describe(ctx: &Ctx<'_>, err: rquickjs::Error) -> String {
    let _ = ctx;
    match err {
        rquickjs::Error::Exception => {
            // 具体信息在 JS 侧的异常对象里，取不到就退回通用描述
            "脚本异常".to_string()
        }
        other => other.to_string(),
    }
}

/// 插件返回值取文本：允许直接返回字符串，也允许返回 `{ text: "..." }`。
/// 翻译与 OCR 两个扩展点共用这个约定。
pub fn text_of(raw: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("插件返回值无法解析: {e}"))?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(o) => o
            .get("text")
            .and_then(|t| t.as_str())
            .map(String::from)
            .ok_or_else(|| "插件返回值缺少 text 字段".to_string()),
        serde_json::Value::Null => Err("插件没有返回内容".into()),
        other => Err(format!("插件返回了不支持的类型: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn script(tag: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suiyi-plugin-js-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("index.js");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn calls_exported_function_with_json() {
        let p = script(
            "basic",
            r#"
            function translate(input) {
              return { text: input.text.toUpperCase(), to: input.to };
            }
            "#,
        );
        let out = call(&p, "translate", r#"{"text":"hello","to":"中文"}"#, &[]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["text"], "HELLO");
        assert_eq!(v["to"], "中文");
    }

    #[test]
    fn plain_string_result_is_fine() {
        let p = script("string", "function translate(i) { return i.text + '!'; }");
        let out = call(&p, "translate", r#"{"text":"hi"}"#, &[]).unwrap();
        assert_eq!(serde_json::from_str::<String>(&out).unwrap(), "hi!");
    }

    #[test]
    fn missing_function_is_reported() {
        let p = script("missing", "function other() {}");
        let err = call(&p, "translate", "{}", &[]).unwrap_err();
        assert!(err.contains("没有导出 translate"), "实际: {err}");
    }

    #[test]
    fn thrown_error_is_reported() {
        let p = script("throw", "function translate() { throw new Error('炸了'); }");
        let err = call(&p, "translate", "{}", &[]).unwrap_err();
        assert!(err.contains("translate() 抛出异常"), "实际: {err}");
    }

    #[test]
    fn syntax_error_is_reported() {
        let p = script("syntax", "function translate( {");
        let err = call(&p, "translate", "{}", &[]).unwrap_err();
        assert!(err.contains("插件脚本执行失败"), "实际: {err}");
    }

    #[test]
    fn clipboard_needs_permission() {
        let p = script(
            "perm",
            "function probe() { return typeof host.clipboard; }",
        );
        // 没申请权限：host 上没有 clipboard
        let out = call(&p, "probe", "{}", &[]).unwrap();
        assert_eq!(serde_json::from_str::<String>(&out).unwrap(), "undefined");

        // 申请后：出现 read / write
        let out = call(&p, "probe", "{}", &["clipboard".to_string()]).unwrap();
        assert_eq!(serde_json::from_str::<String>(&out).unwrap(), "object");
    }

    #[test]
    fn log_is_always_available() {
        let p = script("log", "function ping() { host.log('hello'); return 1; }");
        let out = call(&p, "ping", "{}", &[]).unwrap();
        assert_eq!(out, "1");
    }
}
