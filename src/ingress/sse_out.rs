/// 将 SSE payload 包装为 SSE 帧格式
pub fn frame(payload: &str) -> String {
    format!("data: {payload}\n\n")
}
