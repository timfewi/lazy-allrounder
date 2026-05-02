use crate::{
    error::CoreError,
    ports::{TextGenerationPort, TextOperation, TextToSpeechPort},
};

const MAX_TEXT_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRequest {
    text: String,
}

impl ReadRequest {
    pub fn new(text: impl Into<String>) -> Result<Self, CoreError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(CoreError::EmptyInput);
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(CoreError::InputTooLarge);
        }

        Ok(Self { text })
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskRequest {
    text: String,
    question: String,
}

impl AskRequest {
    pub fn new(text: impl Into<String>, question: impl Into<String>) -> Result<Self, CoreError> {
        let text = text.into();
        let question = question.into();

        if text.trim().is_empty() {
            return Err(CoreError::EmptyInput);
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(CoreError::InputTooLarge);
        }

        if question.trim().is_empty() {
            return Err(CoreError::EmptyQuestion);
        }

        Ok(Self { text, question })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn question(&self) -> &str {
        &self.question
    }
}

pub struct ReadService<T> {
    text_to_speech: T,
}

impl<T> ReadService<T> {
    pub fn new(text_to_speech: T) -> Self {
        Self { text_to_speech }
    }
}

impl<T> ReadService<T>
where
    T: TextToSpeechPort,
{
    pub async fn run(&self, request: ReadRequest, tts_model: &str) -> Result<Vec<u8>, CoreError> {
        self.text_to_speech
            .synthesize(request.text(), tts_model)
            .await
            .map_err(CoreError::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAudio {
    text: String,
    audio: Vec<u8>,
}

impl GeneratedAudio {
    pub fn new(text: String, audio: Vec<u8>) -> Self {
        Self { text, audio }
    }

    pub fn into_parts(self) -> (String, Vec<u8>) {
        (self.text, self.audio)
    }
}

pub struct TransformService<G, T> {
    text_generation: G,
    text_to_speech: T,
}

impl<G, T> TransformService<G, T> {
    pub fn new(text_generation: G, text_to_speech: T) -> Self {
        Self {
            text_generation,
            text_to_speech,
        }
    }
}

impl<G, T> TransformService<G, T>
where
    G: TextGenerationPort,
    T: TextToSpeechPort,
{
    pub async fn explain(
        &self,
        request: ReadRequest,
        text_model: &str,
        tts_model: &str,
    ) -> Result<GeneratedAudio, CoreError> {
        self.run_transform(
            request.text(),
            TextOperation::Explain,
            text_model,
            tts_model,
        )
        .await
    }

    pub async fn summarize(
        &self,
        request: ReadRequest,
        text_model: &str,
        tts_model: &str,
    ) -> Result<GeneratedAudio, CoreError> {
        self.run_transform(
            request.text(),
            TextOperation::Summarize,
            text_model,
            tts_model,
        )
        .await
    }

    pub async fn ask(
        &self,
        request: AskRequest,
        text_model: &str,
        tts_model: &str,
    ) -> Result<GeneratedAudio, CoreError> {
        self.run_transform(
            request.text(),
            TextOperation::Ask {
                question: request.question().to_owned(),
            },
            text_model,
            tts_model,
        )
        .await
    }

    async fn run_transform(
        &self,
        input: &str,
        operation: TextOperation,
        text_model: &str,
        tts_model: &str,
    ) -> Result<GeneratedAudio, CoreError> {
        let generated = self
            .text_generation
            .generate(input, &operation, text_model)
            .await?;
        let audio = self
            .text_to_speech
            .synthesize(&generated, tts_model)
            .await?;
        Ok(GeneratedAudio::new(generated, audio))
    }
}

#[cfg(test)]
mod tests {
    use super::{AskRequest, ReadRequest};
    use crate::error::CoreError;

    #[test]
    fn read_request_rejects_blank_input() {
        let error = ReadRequest::new("   ").expect_err("blank input should fail");
        assert!(matches!(error, CoreError::EmptyInput));
    }

    #[test]
    fn ask_request_rejects_blank_question() {
        let error = AskRequest::new("hello", "  ").expect_err("blank question should fail");
        assert!(matches!(error, CoreError::EmptyQuestion));
    }

    #[test]
    fn read_request_rejects_large_input() {
        let error = ReadRequest::new("a".repeat(20_001)).expect_err("large input should fail");
        assert!(matches!(error, CoreError::InputTooLarge));
    }
}
