mod commands;
mod ollama;

use std::collections::HashMap;
use std::env;

use ollama_rs::generation::chat::ChatMessage;

use serenity::all::{
    ActivityData, Channel, ChannelId, Command, CreateAllowedMentions, CreateAttachment,
    CreateMessage, GetMessages, Interaction, MessageReference, MessageReferenceKind,
};

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

use crate::commands::{handle_command, parse_commands};
use crate::ollama::{
    create_chat_history, desire_to_respond, get_llm_response, load_chat_history, save_chat_history,
};

struct ChatHistory;
struct Handler;

impl TypeMapKey for ChatHistory {
    type Value = HashMap<u64, Vec<ChatMessage>>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            let possible_command = parse_commands(&command);
            match possible_command {
                Some(command_type) => handle_command(ctx.clone(), &command, command_type).await,
                _ => (),
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.id == ctx.cache.current_user().id {
            return;
        }

        let channel = msg.channel(&ctx).await.unwrap();
        let is_dm = match channel {
            Channel::Private(_) => true,
            _ => false,
        };

        let is_registered = {
            let data = ctx.data.read().await;
            get_chat_history(&data, msg.channel_id.get()).is_some()
        };
        if !is_dm && !is_registered {
            return;
        }
        let user_message: String = (match msg.author.global_name {
            Some(ref name) => name,
            None => &msg.author.name,
        })
        .to_string()
            + " says: "
            + msg.content.as_str();
        let want_to_reply = if is_dm || msg.mentions_me(&ctx.http).await.unwrap() {
            true
        } else {
            ctx.set_activity(Some(ActivityData::custom("Thinking...")));
            let mut data = ctx.data.write().await;
            let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
            desire_to_respond(chat_history, user_message.clone()).await
        };
        if want_to_reply {
            send_message(ctx.clone(), msg).await;
        } else {
            let mut data = ctx.data.write().await;
            let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
            chat_history.push(ChatMessage::user(user_message));
        }
        ctx.set_activity(Some(ActivityData::custom("The self splinters")));

        save_chat_history(ctx.data.write().await.get_mut::<ChatHistory>().unwrap()).await
    }

    async fn ready(&self, ctx: Context, _: serenity::model::gateway::Ready) {
        ctx.set_activity(Some(ActivityData::custom("The self splinters")));
        for command in commands::register_commands() {
            let _ = Command::create_global_command(&ctx.http, command).await;
        }
    }
}

async fn get_discord_messages(
    ctx: impl serenity::http::CacheHttp,
    channel_id: ChannelId,
) -> Vec<Message> {
    let chat_history_builder = GetMessages::new().limit(100);
    channel_id
        .messages(ctx, chat_history_builder)
        .await
        .unwrap()
}

async fn send_message(ctx: Context, msg: Message) {
    let typing = ctx.http.start_typing(msg.channel_id);
    let mut data = ctx.data.write().await;
    let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
    chat_history.push(ChatMessage::user(if msg.guild_id == None {
        msg.content.clone()
    } else {
        (match msg.author.global_name.clone() {
            Some(name) => name,
            None => msg.author.name.clone(),
        }) + " says: "
            + msg.content.as_str()
    }));
    let response = get_llm_response(chat_history).await;

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
