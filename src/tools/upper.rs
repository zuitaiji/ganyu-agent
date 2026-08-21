//! `ganyu tool upper`：读取 stdin 全部内容并转为大写后输出。
//! 等价 `plugins/upper.py`。

use ganyu_agent::error::GanyuResult;

pub fn run(_args: &[String]) -> GanyuResult<()> {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return Err(ganyu_agent::error::GanyuError::Forbidden("读取 stdin 失败".into()));
    }
    print!("{}", buf.to_uppercase());
    Ok(())
}
