use async_trait::async_trait;

use crate::error::PortError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextOperation {
    Explain,
    Summarize,
    Ask { question: String },
}

#[async_trait]
pub trait TextGenerationPort: Send + Sync {
    async fn generate(
        &self,
        input: &str,
        operation: &TextOperation,
        model: &str,
    ) -> Result<String, PortError>;
}

#[async_trait]
pub trait TextToSpeechPort: Send + Sync {
    async fn synthesize(&self, text: &str, model: &str) -> Result<Vec<u8>, PortError>;
}
