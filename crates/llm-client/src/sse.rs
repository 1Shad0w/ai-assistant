//! Parser incremental de Server-Sent Events (SSE).

/// Acumula bytes arbitrarios y emite el campo `data` de cada evento completo.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseParser {
    /// Crea un parser vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Alimenta bytes y devuelve las cargas `data` de los eventos completados.
    ///
    /// Los eventos pueden llegar fragmentados en cualquier punto.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();

        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = self.buffer.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw);
            let line = line.trim_end_matches(['\n', '\r']);

            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    events.push(self.data_lines.join("\n"));
                    self.data_lines.clear();
                }
            } else if line.starts_with(':') {
                // Comentario / keep-alive.
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data_lines
                    .push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            }
            // Otros campos (event, id, retry) no se usan.
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_fragmented_events() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: {\"a\"").is_empty());
        let ev = p.push(b":1}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(ev, vec!["{\"a\":1}".to_owned(), "[DONE]".to_owned()]);
    }

    #[test]
    fn ignores_comments_and_joins_multiline() {
        let mut p = SseParser::new();
        let ev = p.push(b": ping\n\ndata: a\ndata: b\n\n");
        assert_eq!(ev, vec!["a\nb".to_owned()]);
    }
}
