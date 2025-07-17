use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, MessageRole, request::ChatMessageRequest},
};
use serenity::all::{Context, Message};

use crate::{ChatHistory, create_chat_history, get_older_discord_messages};

const MODEL: &str = "llama3.1:latest";

pub enum Command {
    Register,
    Unregister,
    Amnesia,
    Nuke,
    SuperNuke,
    SetPrompt(String),
    Regenerate,
}

pub fn parse_commands(message: &str) -> Option<Command> {
    Some(match message {
        "!register" => Command::Register,
        "!unregister" => Command::Unregister,
        "!amnesia" => Command::Amnesia,
        "!nuke" => Command::Nuke,
        "!supernuke" => Command::SuperNuke,
        s if s.starts_with("!setprompt ") => {
            Command::SetPrompt(s.split_once("!setprompt ").unwrap().1.to_string())
        }
        "!regenerate" => Command::Regenerate,
        _ => return None,
    })
}

pub async fn handle_command(ctx: Context, msg: Message, command: Command, is_dm: bool) {
    match command {
        Command::Register => register(ctx, msg, is_dm).await,
        Command::Unregister => unregister(ctx, msg, is_dm).await,
        Command::Amnesia => amnesia(ctx, msg).await,
        Command::Nuke => nuke(ctx, msg).await,
        Command::SuperNuke => super_nuke(ctx, msg).await,
        Command::SetPrompt(prompt) => set_prompt(ctx, msg, prompt).await,
        Command::Regenerate => regenerate(ctx, msg).await,
    }
}

async fn register(ctx: Context, msg: Message, is_dm: bool) {
    if is_dm {
        let _ = msg
            .reply(&ctx.http, "I will always reply to our private messages!")
            .await;
        return;
    }
    let mut data = ctx.data.write().await;
    let chat_history = data.get_mut::<ChatHistory>().unwrap();
    if !chat_history.contains_key(&msg.channel_id.get()) {
        chat_history.insert(
            msg.channel_id.get(),
            create_chat_history(&mut Ollama::default()).await,
        );
        let _ = msg
            .reply(&ctx.http, "I will now respond to messages in this channel!")
            .await;
    }
}

async fn unregister(ctx: Context, msg: Message, is_dm: bool) {
    if is_dm {
        let _ = msg
            .reply(
                &ctx.http,
                "Sorry, you can't unregister in DMs\nBut, if you want to reset the chat you can use: `!amnesia`",
            )
            .await;
        return;
    }
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        chat_history.remove(&msg.channel_id.get());
    }
    let _ = msg.reply(&ctx.http, "Goodbye!").await;
}

async fn amnesia(ctx: Context, msg: Message) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        chat_history.insert(
            msg.channel_id.get(),
            create_chat_history(&mut Ollama::default()).await,
        );
    }
    let _ = msg.reply(&ctx.http, "Chat history has been reset!").await;
}

async fn nuke(ctx: Context, msg: Message) {
    let discord_chat_history = get_older_discord_messages(&ctx.http, msg.id, msg.channel_id).await;
    tokio::spawn(async move {
        for message in discord_chat_history {
            if message.author.id == ctx.cache.current_user().id {
                let _ = message.delete(&ctx.http).await;
            }
        }
        println!("Nuke done.");
    });
}

async fn super_nuke(ctx: Context, msg: Message) {
    tokio::spawn(async move {
        let discord_chat_history =
            get_older_discord_messages(&ctx.http, msg.id, msg.channel_id).await;
        let _ = msg.delete(&ctx.http).await;
        for message in discord_chat_history {
            let _ = message.delete(&ctx.http).await;
        }
        println!("Super nuke done.");
    });
}

async fn set_prompt(ctx: Context, msg: Message, prompt: String) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        let chat_history = chat_history
            .entry(msg.channel_id.get())
            .or_insert(create_chat_history(&mut Ollama::default()).await);
        chat_history.retain(|m| m.role != MessageRole::System);
        let _ = Ollama::default()
            .send_chat_messages_with_history(
                chat_history,
                ChatMessageRequest::new(MODEL.to_string(), vec![ChatMessage::system(prompt)]),
            )
            .await;
        chat_history.pop(); // remove empty LLM response
        let system_prompt = chat_history.pop().unwrap(); // Get the system prompt and move it to the start of the chat history
        chat_history.insert(0, system_prompt);
    }
    let _ = msg.reply(&ctx.http, "System prompt set!").await;
}

async fn regenerate(ctx: Context, msg: Message) {
    let mut data = ctx.data.write().await;
    let chat_history = data
        .get_mut::<ChatHistory>()
        .unwrap()
        .entry(msg.channel_id.get())
        .or_insert(create_chat_history(&mut Ollama::default()).await);

    chat_history.pop();
    let response = Ollama::default()
        .send_chat_messages_with_history(
            chat_history,
            ChatMessageRequest::new(MODEL.to_string(), vec![]),
        )
        .await;
    let _ = msg
        .channel_id
        .say(&ctx.http, response.unwrap().message.content)
        .await;
}
