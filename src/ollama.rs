use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
};
use serenity::all::Message;

static DEFAULT_PROMPT: &str = include_str!("prompt.txt");

pub fn set_system_prompt(chat_history: &mut Vec<ChatMessage>, prompt: &str) -> ChatMessage {
    //Sets a new system prompt, returning the old one
    let old_prompt = chat_history[0].to_owned(); // We always keep the system prompt as the first message
    chat_history[0] = ChatMessage::system(prompt.to_string());
    old_prompt
}

pub async fn get_llm_response(
    chat_history: &mut Vec<ChatMessage>,
    msg: &Message,
    model: &str,
) -> String {
    Ollama::default()
        .send_chat_messages_with_history(
            chat_history,
            ChatMessageRequest::new(
                model.to_string(),
                vec![ChatMessage::user(if msg.guild_id == None {
                    msg.content.clone()
                } else {
                    (match msg.author.global_name.clone() {
                        Some(name) => name,
                        None => msg.author.name.clone(),
                    }) + " says: "
                        + msg.content.as_str()
                })],
            ),
        )
        .await
        .unwrap()
        .message
        .content
}

pub async fn create_chat_history() -> Vec<ChatMessage> {
    vec![ChatMessage::system(DEFAULT_PROMPT.to_string())]
}
