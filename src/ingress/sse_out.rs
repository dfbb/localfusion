/// Wraps an SSE payload into SSE frame format
pub fn frame(payload: &str) -> String {
    format!("data: {payload}\n\n")
}
