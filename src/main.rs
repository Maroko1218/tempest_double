use std::collections::HashMap;
use std::env;

use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;

use serenity::all::{
    ActivityData, Channel, CreateAllowedMentions, CreateAttachment, CreateMessage,
    MessageReference, MessageReferenceKind,
};

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;

static DEFAULT_PROMPT: &str = include_str!("prompt.txt");
const MODEL: &str = "llama3.1:latest";
struct ChatHistory;
struct Handler;

impl TypeMapKey for ChatHistory {
    type Value = HashMap<u64, Vec<ChatMessage>>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.id == ctx.cache.current_user().id {
            return;
        }

        let channel = msg.channel(&ctx).await.unwrap();
        let is_dm = match channel {
            Channel::Private(_) => true,
            _ => false,
        };
        if !is_dm && channel.id().get() != 1383742335027773540 {
            return;
        }

        if msg.content.starts_with("!") {
            handle_command(ctx, msg).await;
        } else {
            send_message(ctx, msg, is_dm).await;
        }
    }

    async fn ready(&self, ctx: Context, _: serenity::model::gateway::Ready) {
        ctx.set_activity(Some(ActivityData::custom("The self splinters")));
    }
}

async fn handle_command(ctx: Context, msg: Message) {
    let mut ollama = Ollama::default();
    if msg.content.starts_with("!amnesia") {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        let mut old_vec = chat_history.remove(&msg.channel_id.get());
        old_vec.take();
        drop(old_vec);
        chat_history.insert(msg.channel_id.get(), create_chat_history(&mut ollama).await);
        let _ = msg.reply(&ctx.http, "Chat history has been reset!").await;
    } else if msg.content.starts_with("!setprompt ") {
        let mut data = ctx.data.write().await;
        let all_chat_history = data.get_mut::<ChatHistory>().unwrap();
        let chat_history = all_chat_history
            .entry(msg.channel_id.get())
            .or_insert(create_chat_history(&mut ollama).await);
        let _ = ollama
            .send_chat_messages_with_history(
                chat_history,
                ChatMessageRequest::new(
                    MODEL.to_string(),
                    vec![ChatMessage::system(
                        msg.content.split_once("!setprompt ").unwrap().1.to_string(),
                    )],
                ),
            )
            .await;
        let _ = msg.reply(&ctx.http, "System prompt set!").await;
    }
}

async fn send_message(ctx: Context, msg: Message, is_dm: bool) {
    let typing = ctx.http.start_typing(msg.channel_id);
    let mut ollama = Ollama::default();
    let mut data = ctx.data.write().await;
    let all_chat_history = data.get_mut::<ChatHistory>().unwrap();
    let chat_history = all_chat_history
        .entry(msg.channel_id.get())
        .or_insert(create_chat_history(&mut ollama).await);

    if let Ok(res) = ollama
        .send_chat_messages_with_history(
            chat_history,
            ChatMessageRequest::new(
                MODEL.to_string(),
                vec![if is_dm {
                    ChatMessage::user(msg.content)
                } else {
                    ChatMessage::user(
                        match msg.author.global_name {
                            Some(name) => name,
                            None => msg.author.name,
                        } + " says: "
                            + msg.content.as_str(),
                    )
                }],
            ),
        )
        .await
    {
        let message_builder = CreateMessage::new()
            .reference_message(
                MessageReference::new(MessageReferenceKind::Default, msg.channel_id)
                    .message_id(msg.id),
            )
            .allowed_mentions(CreateAllowedMentions::new().empty_users().empty_roles()); // Make the reference not mention the user
        let response = if res.message.content.len() > 2000 {
            message_builder.add_file(CreateAttachment::bytes(res.message.content, "reply.txt"))
        } else {
            message_builder.content(res.message.content)
        };
        if let Err(why) = msg.channel_id.send_message(&ctx.http, response).await {
            println!("Couldn't send message: {why:?}");
        }
    }
    typing.stop();
}

async fn create_chat_history(ollama: &mut Ollama) -> Vec<ChatMessage> {
    let mut history = vec![];
    if let Err(why) = ollama
        .send_chat_messages_with_history(
            &mut history,
            ChatMessageRequest::new(
                MODEL.to_string(),
                vec![ChatMessage::system(DEFAULT_PROMPT.to_string())],
            ),
        )
        .await
    {
        println!("Couldn't set system prompt: {why:?}");
    }
    history
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().expect("Failed to load .env file");
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let intents = GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGES;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("Err creating client");
    {
        let mut data = client.data.write().await;
        data.insert::<ChatHistory>(HashMap::default());
    }

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
