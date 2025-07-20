use serenity::all::{Context, Message};

use crate::{
    ChatHistory, create_chat_history, get_chat_history, get_mutable_chat_history,
    get_older_discord_messages,
    ollama::{get_llm_response, set_system_prompt},
};

const MODEL: &str = "llama3.1:latest";

pub enum Command<'a> {
    Register,
    Unregister,
    Amnesia,
    Nuke,
    SuperNuke,
    SetPrompt(&'a str),
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
            Command::SetPrompt(s.split_once("!setprompt ").unwrap().1)
        }
        "!regenerate" => Command::Regenerate,
        _ => return None,
    })
}

pub async fn handle_command<'a>(ctx: Context, msg: Message, command: Command<'a>) {
    match command {
        Command::Register => register(ctx, &msg).await,
        Command::Unregister => unregister(ctx, &msg).await,
        Command::Amnesia => amnesia(ctx, &msg).await,
        Command::Nuke => nuke(ctx, msg).await,
        Command::SuperNuke => super_nuke(ctx, msg).await,
        Command::SetPrompt(prompt) => set_prompt(ctx, &msg, prompt).await,
        Command::Regenerate => regenerate(ctx, &msg).await,
    }
}

async fn register(ctx: Context, msg: &Message) {
    if msg.guild_id == None {
        let _ = msg
            .reply(&ctx.http, "I will always reply to our private messages!")
            .await;
        return;
    }
    let is_already_registered = {
        let data = ctx.data.read().await;
        get_chat_history(&data, msg.channel_id.get()).is_some()
    };
    if !is_already_registered {
        let mut data = ctx.data.write().await;
        get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
        let _ = msg
            .reply(&ctx.http, "I will now respond to messages in this channel!")
            .await;
    }
}

async fn unregister(ctx: Context, msg: &Message) {
    if msg.guild_id == None {
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

async fn amnesia(ctx: Context, msg: &Message) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        chat_history.insert(msg.channel_id.get(), create_chat_history().await);
    }
    let _ = msg.reply(&ctx.http, "Chat history has been reset!").await;
}

async fn nuke(ctx: Context, msg: Message) {
    tokio::spawn(async move {
        let discord_chat_history =
            get_older_discord_messages(&ctx.http, msg.id, msg.channel_id).await;
        for message in discord_chat_history
            .iter()
            .filter(|m| m.author.id == ctx.cache.current_user().id)
        {
            let _ = message.delete(&ctx.http).await;
        }
        println!("Nuke done.");
    });
}

async fn super_nuke(ctx: Context, msg: Message) {
    tokio::spawn(async move {
        let discord_chat_history =
            get_older_discord_messages(&ctx.http, msg.id, msg.channel_id).await;
        let _ = msg
            .channel_id
            .delete_messages(&ctx.http, discord_chat_history)
            .await;
        let _ = msg.delete(&ctx.http).await;
    });
}

async fn set_prompt(ctx: Context, msg: &Message, prompt: &str) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
        set_system_prompt(chat_history, prompt);
    }
    let _ = msg.reply(&ctx.http, "System prompt set!").await;
}

async fn regenerate(ctx: Context, msg: &Message) {
    let response = {
        let mut data = ctx.data.write().await;
        let chat_history = get_mutable_chat_history(&mut data, msg.channel_id.get()).await;
        chat_history.pop();
        get_llm_response(chat_history, msg, MODEL).await
    };
    let _ = msg.channel_id.say(&ctx.http, response).await;
}
