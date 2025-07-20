mod commands;

use std::collections::HashMap;
use std::env;

use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;

use serenity::all::{
    ActivityData, Channel, ChannelId, CreateAllowedMentions, CreateAttachment, CreateMessage,
    GetMessages, MessageId, MessageReference, MessageReferenceKind,
};

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;

static DEFAULT_PROMPT: &str = include_str!("prompt.txt");
const MODEL: &str = "llama3.1:latest";
const EVALUATOR_MODEL: &str = "gemma3:4b";
const EVALUATOR_PROMPT: &str =
    "Decide if you want to reply to this conversation. Answer only with a Yes or a No.";

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
        let possible_command = commands::parse_commands(msg.content.as_str());

        let is_registered = {
            let data = ctx.data.write().await;
            data.get::<ChatHistory>()
                .unwrap()
                .contains_key(&channel.id().get())
        };
        if !is_dm
            && !is_registered
            && !matches!(possible_command, Some(commands::Command::Register))
        {
            return;
        }

        if possible_command.is_some() {
            commands::handle_command(ctx.clone(), msg, possible_command.unwrap(), is_dm).await;
        } else {
            let want_to_reply: bool = desire_to_respond(ctx.clone(), msg.clone(), is_dm).await;
            println!("{}", want_to_reply);
            if want_to_reply {
                send_message(ctx.clone(), msg, is_dm).await;
            } else {
                let mut data = ctx.data.write().await;
                data.get_mut::<ChatHistory>()
                    .unwrap()
                    .entry(msg.channel_id.get())
                    .and_modify(|history| {
                        history.push(ChatMessage::user(if is_dm {
                            msg.content
                        } else {
                            (match msg.author.global_name {
                                Some(name) => name,
                                None => msg.author.name,
                            }) + " says: "
                                + msg.content.as_str()
                        }))
                    });
            }
        }
    }

    async fn ready(&self, ctx: Context, _: serenity::model::gateway::Ready) {
        ctx.set_activity(Some(ActivityData::custom("The self splinters")));
    }
}

async fn desire_to_respond(ctx: Context, msg: Message, is_dm: bool) -> bool {
    if is_dm {
        return is_dm;
    }
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        let chat_history = chat_history
            .entry(msg.channel_id.get())
            .or_insert(create_chat_history().await);
        let original_system_prompt = chat_history.remove(0);
        let _ = Ollama::default()
            .send_chat_messages_with_history(
                chat_history,
                ChatMessageRequest::new(
                    MODEL.to_string(),
                    vec![ChatMessage::system(EVALUATOR_PROMPT.to_owned())],
                ),
            )
            .await;
        chat_history.pop(); // remove empty LLM response
        let evaluator_response = Ollama::default()
            .send_chat_messages_with_history(
                chat_history,
                ChatMessageRequest::new(
                    EVALUATOR_MODEL.to_owned(),
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
            .unwrap();
        println!("{:?}", chat_history.pop()); //Yes or no (hopefully)
        chat_history.pop(); //User message
        chat_history.pop(); //System prompt
        chat_history.insert(0, original_system_prompt);
        evaluator_response
            .message
            .content
            .to_ascii_lowercase()
            .contains("yes")
    }
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

async fn send_message(ctx: Context, msg: Message, is_dm: bool) {
    let typing = ctx.http.start_typing(msg.channel_id);
    let mut ollama = Ollama::default();
    let mut data = ctx.data.write().await;
    let all_chat_history = data.get_mut::<ChatHistory>().unwrap();
    let chat_history = all_chat_history
        .entry(msg.channel_id.get())
        .or_insert(create_chat_history().await);

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

async fn create_chat_history() -> Vec<ChatMessage> {
    let mut history = vec![];
    if let Err(why) = Ollama::default()
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
