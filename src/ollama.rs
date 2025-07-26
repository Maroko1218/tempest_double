use std::collections::HashMap;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
};

static DEFAULT_PROMPT: &str = include_str!("prompt.txt");
const HISTORY_FILE_NAME: &str = "history.json";
const MODEL: &str = "llama3.1:latest";
const EVALUATOR_PROMPT: &str = "You are a helpful assistant. Your sole task is to determine if you should continue this conversation. Respond only with 'Yes' or 'No'. Do not provide any additional explanation, greetings, or commentary. Indicate your intent with a simple 'Yes' if you wish to continue, and 'No' if you do not.";

pub fn set_system_prompt(chat_history: &mut Vec<ChatMessage>, prompt: &str) -> ChatMessage {
    //Sets a new system prompt, returning the old one
    let old_prompt = chat_history[0].to_owned(); // We always keep the system prompt as the first message
    chat_history[0] = ChatMessage::system(prompt.to_string());
    old_prompt
}

pub async fn get_llm_response(chat_history: &mut Vec<ChatMessage>) -> String {
    Ollama::default()
        .send_chat_messages_with_history(
            chat_history,
            ChatMessageRequest::new(MODEL.to_string(), vec![]),
        )
        .await
        .unwrap()
        .message
        .content
}

pub async fn create_chat_history() -> Vec<ChatMessage> {
    vec![ChatMessage::system(DEFAULT_PROMPT.to_string())]
}

pub async fn save_chat_history(all_history: &HashMap<u64, Vec<ChatMessage>>) {
    let mut file = File::create(HISTORY_FILE_NAME).await.unwrap();
    file.write_all(serde_json::to_string(&all_history).unwrap().as_bytes())
        .await
        .unwrap();
}

pub async fn load_chat_history() -> HashMap<u64, Vec<ChatMessage>> {
    let file = File::open(HISTORY_FILE_NAME).await;
    match file {
        Ok(mut file) => {
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await.unwrap();
            serde_json::from_str(&buffer).unwrap()
        }
        Err(_) => HashMap::<u64, Vec<ChatMessage>>::new(),
    }
}

pub async fn desire_to_respond(chat_history: &mut Vec<ChatMessage>, message: String) -> bool {
    let original_system_prompt = set_system_prompt(chat_history, EVALUATOR_PROMPT);
    chat_history.push(ChatMessage::user(message));
    let evaluator_response = get_llm_response(chat_history).await;
    println!("{:?}", chat_history.pop()); //Yes or no (hopefully)
    chat_history.pop(); //User message
    chat_history[0] = original_system_prompt;
    evaluator_response.to_ascii_lowercase().contains("yes")
}
