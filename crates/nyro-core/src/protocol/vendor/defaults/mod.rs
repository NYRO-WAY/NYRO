//! Family-level no-op fallbacks. These match any provider whose
//! protocol belongs to the family but whose vendor/channel does not
//! have a more specific registration.

mod anthropic;
mod google;
mod openai;

pub use anthropic::AnthropicDefault;
pub use google::GoogleDefault;
pub use openai::OpenAiDefault;
