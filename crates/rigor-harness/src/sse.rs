pub enum SseFormat {
    Anthropic,
    OpenAI,
}

pub fn parse_sse_events(_body: &str) -> Vec<String> {
    todo!()
}

pub fn extract_text_from_sse(_events: &[String], _format: SseFormat) -> String {
    todo!()
}

pub fn anthropic_sse_chunks(_text: &str) -> Vec<String> {
    todo!()
}

pub fn openai_sse_chunks(_text: &str) -> Vec<String> {
    todo!()
}
