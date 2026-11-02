//! Troceado de texto en frases para sintetizar voz con baja latencia.

/// Caracteres que cierran una frase.
const BOUNDARIES: [char; 4] = ['.', '!', '?', '…'];

/// Trocea texto que llega en deltas (streaming del LLM) en frases completas.
///
/// Una frase se cierra tras `. ! ? …` seguidos de espacio, o en un salto de
/// línea. Así `3.5` o `ej.com` no se cortan.
#[derive(Debug, Default)]
pub struct SentenceChunker {
    buffer: String,
}

impl SentenceChunker {
    /// Crea un chunker vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade texto y devuelve las frases completadas.
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buffer.push_str(delta);
        let mut out = Vec::new();
        while let Some(end) = self.find_boundary() {
            let rest = self.buffer.split_off(end);
            let sentence = std::mem::replace(&mut self.buffer, rest);
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                out.push(sentence.to_owned());
            }
        }
        out
    }

    /// Devuelve el texto restante al terminar el flujo.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buffer);
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_owned())
    }

    /// Índice (exclusivo) donde termina la primera frase completa.
    fn find_boundary(&self) -> Option<usize> {
        let mut iter = self.buffer.char_indices().peekable();
        while let Some((i, c)) = iter.next() {
            if c == '\n' {
                return Some(i + 1);
            }
            if BOUNDARIES.contains(&c) {
                if let Some(&(_, next)) = iter.peek() {
                    if next.is_whitespace() {
                        return Some(i + c.len_utf8());
                    }
                }
            }
        }
        None
    }
}

/// Trocea un texto completo en frases.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut chunker = SentenceChunker::new();
    let mut out = chunker.push(text);
    out.extend(chunker.finish());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_complete_text() {
        assert_eq!(
            split_sentences("Hola. ¿Qué tal? Valor 3.5 ok!"),
            vec!["Hola.", "¿Qué tal?", "Valor 3.5 ok!"]
        );
    }

    #[test]
    fn streams_across_deltas() {
        let mut c = SentenceChunker::new();
        assert!(c.push("Hola mu").is_empty());
        assert!(c.push("ndo.").is_empty()); // aún no sabemos si sigue un decimal
        assert_eq!(c.push(" Siguiente"), vec!["Hola mundo."]);
        assert_eq!(c.finish(), Some("Siguiente".to_owned()));
        assert_eq!(c.finish(), None);
    }

    #[test]
    fn newline_closes_sentence() {
        assert_eq!(split_sentences("uno\ndos"), vec!["uno", "dos"]);
    }
}
