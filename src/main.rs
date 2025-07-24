mod commands;
mod ollama;

use std::collections::HashMap;
use std::env;

use ollama_rs::generation::chat::ChatMessage;

use serenity::all::{
    ActivityData, Channel, ChannelId, CreateAllowedMentions, CreateAttachment, CreateMessage,
    GetMessages, MessageId, MessageReference, MessageReferenceKind,
};

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

use crate::commands::{Command, handle_command, parse_commands};
use crate::ollama::{
    create_chat_history, get_llm_response, load_chat_history, save_chat_history, set_system_prompt,
};

const MODEL: &str = "llama3.1:latest";
const EVALUATOR_PROMPT: &str = "You are a helpful assistant. Your sole task is to determine if you should continue this conversation. Respond only with 'Yes' or 'No'. Do not provide any additional explanation, greetings, or commentary. Indicate your intent with a simple 'Yes' if you wish to continue, and 'No' if you do not.";

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
        let possible_command = parse_commands(msg.content.as_str());

        let is_registered = {
            let data = ctx.data.read().await;
            get_chat_history(&data, msg.channel_id.get()).is_some()
        };
        if !is_dm && !is_registered && !matches!(possible_command, Some(Command::Register)) {
            return;
        }

        if possible_command.is_some() {
            handle_command(ctx.clone(), msg.clone(), possible_command.unwrap()).await;
        } else {
            let want_to_reply = if is_dm || msg.mentions_me(&ctx.http).await.unwrap() {
                true
            } else {
                desire_to_respond(&ctx, msg.clone()).await
            };
            if want_to_reply {
                send_message(ctx.clone(), msg).await;
            } else {
                let mut data = ctx.data.write().await;
                let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
                chat_history.push(ChatMessage::user(if is_dm {
                    msg.content
                } else {
                    (match msg.author.global_name {
                        Some(name) => name,
                        None => msg.author.name,
                    }) + " says: "
                        + msg.content.as_str()
                }));
            }
            ctx.set_activity(Some(ActivityData::custom("The self splinters")));
        }
        save_chat_history(ctx.data.write().await.get_mut::<ChatHistory>().unwrap()).await
    }

    async fn ready(&self, ctx: Context, _: serenity::model::gateway::Ready) {
        ctx.set_activity(Some(ActivityData::custom("The self splinters")));
    }
}

async fn desire_to_respond(ctx: &Context, msg: Message) -> bool {
    ctx.set_activity(Some(ActivityData::custom("Thinking...")));
    let mut data = ctx.data.write().await;
    let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
    let original_system_prompt = set_system_prompt(chat_history, EVALUATOR_PROMPT);
    let evaluator_response = get_llm_response(chat_history, &msg, MODEL).await;
    println!("{:?}", chat_history.pop()); //Yes or no (hopefully)
    chat_history.pop(); //User message
    chat_history[0] = original_system_prompt;
    evaluator_response.to_ascii_lowercase().contains("yes")
}

async fn get_older_discord_messages(
    ctx: impl serenity::http::CacheHttp,
    msg_id: MessageId,
    channel_id: ChannelId,
) -> Vec<Message> {
    let chat_history_builder = GetMessages::new().before(msg_id).limit(100);
    channel_id
        .messages(ctx, chat_history_builder)
        .await
        .unwrap()
}

async fn send_message(ctx: Context, msg: Message) {
    let typing = ctx.http.start_typing(msg.channel_id);
    let mut data = ctx.data.write().await;
    let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;

    let response = get_llm_response(chat_history, &msg, MODEL).await;

    let chat_history_builder = GetMessages::new().after(msg.id);
    let new_messages = msg
        .channel_id
        .messages(&ctx.http, chat_history_builder)
        .await
        .unwrap();
    let message_builder = CreateMessage::new();
    let message_builder = if new_messages.len() >= 1 {
        message_builder
            .reference_message(
                MessageReference::new(MessageReferenceKind::Default, msg.channel_id)
                    .message_id(msg.id),
            )
            .allowed_mentions(CreateAllowedMentions::new().empty_users().empty_roles()) // Make the reference not mention the user
    } else {
        message_builder
    };
    let response = if response.len() > 2000 {
        message_builder.add_file(CreateAttachment::bytes(response, "reply.txt"))
    } else {
        message_builder.content(response)
    };
    if let Err(why) = msg.channel_id.send_message(&ctx.http, response).await {
        println!("Couldn't send message: {why:?}");
    }

    typing.stop();
}

async fn get_mutable_chat_history<'a>(
    data: &'a mut RwLockWriteGuard<'_, TypeMap>,
    id: u64,
) -> &'a mut Vec<ChatMessage> {
    data.get_mut::<ChatHistory>()
        .unwrap()
        .entry(id)
        .or_insert(create_chat_history().await)
}

fn get_chat_history<'a>(
    data: &'a RwLockReadGuard<'_, TypeMap>,
    id: u64,
) -> Option<&'a Vec<ChatMessage>> {
    data.get::<ChatHistory>().unwrap().get(&id)
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
        data.insert::<ChatHistory>(load_chat_history().await);
    }

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
