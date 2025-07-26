use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage,
};

use crate::{
    ChatHistory, create_chat_history, get_chat_history, get_discord_messages,
    get_mutable_chat_history,
    ollama::{get_llm_response, set_system_prompt},
};

pub enum Command<'a> {
    Register,
    Unregister,
    Amnesia,
    Nuke,
    SuperNuke,
    SetPrompt(&'a str),
    Regenerate,
}

pub fn parse_commands(command: &CommandInteraction) -> Option<Command> {
    Some(match command.data.name.as_str() {
        "register" => Command::Register,
        "unregister" => Command::Unregister,
        "amnesia" => Command::Amnesia,
        "nuke" => Command::Nuke,
        "supernuke" => Command::SuperNuke,
        "setprompt" => Command::SetPrompt(command.data.options[0].value.as_str().unwrap()),
        "regenerate" => Command::Regenerate,
        _ => return None,
    })
}

pub async fn handle_command<'a>(
    ctx: Context,
    command: &CommandInteraction,
    command_type: Command<'a>,
) {
    match command_type {
        Command::Register => register(ctx, command).await,
        Command::Unregister => unregister(ctx, command).await,
        Command::Amnesia => amnesia(ctx, command).await,
        Command::Nuke => nuke(ctx, command.clone()).await,
        Command::SuperNuke => super_nuke(ctx, command).await,
        Command::SetPrompt(prompt) => set_prompt(ctx, command, prompt).await,
        Command::Regenerate => regenerate(ctx, command).await,
    }
}

async fn register(ctx: Context, command: &CommandInteraction) {
    if command.guild_id == None {
        reply_to_command(
            &ctx,
            &command,
            "I will always reply to our private messages!",
        )
        .await;
        return;
    }
    let is_already_registered = {
        let data = ctx.data.read().await;
        get_chat_history(&data, command.channel_id.get()).is_some()
    };
    if !is_already_registered {
        let mut data = ctx.data.write().await;
        get_mutable_chat_history(&mut data, command.channel_id.get()).await;
        reply_to_command(
            &ctx,
            command,
            "I will now respond to messages in this channel!",
        )
        .await;
    } else {
        reply_to_command(&ctx, command, "Already registered!").await;
    }
}

async fn unregister(ctx: Context, command: &CommandInteraction) {
    if command.guild_id == None {
        reply_to_command(
                &ctx,
                command,
                "Sorry, you can't unregister in DMs\nBut, if you want to reset the chat you can use: `!amnesia`",
            )
            .await;
        return;
    }
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        chat_history.remove(&command.channel_id.get());
    }
    reply_to_command(&ctx, command, "Goodbye!").await;
}

async fn amnesia(ctx: Context, command: &CommandInteraction) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = data.get_mut::<ChatHistory>().unwrap();
        chat_history.insert(command.channel_id.get(), create_chat_history().await);
    }
    reply_to_command(&ctx, command, "Chat history has been reset!").await;
}

async fn nuke(ctx: Context, command: CommandInteraction) {
    tokio::spawn(async move {
        let discord_chat_history = get_discord_messages(&ctx.http, command.channel_id).await;
        for message in discord_chat_history
            .iter()
            .filter(|m| m.author.id == ctx.cache.current_user().id)
        {
            let _ = message.delete(&ctx.http).await;
        }
        reply_to_command(&ctx, &command, "Nuke done.").await;
    });
}

async fn super_nuke(ctx: Context, command: &CommandInteraction) {
    let discord_chat_history = get_discord_messages(&ctx.http, command.channel_id).await;
    let _ = command
        .channel_id
        .delete_messages(&ctx.http, discord_chat_history)
        .await;
    reply_to_command(&ctx, command, "Nuke done.").await;
}

async fn set_prompt(ctx: Context, command: &CommandInteraction, prompt: &str) {
    {
        let mut data = ctx.data.write().await;
        let chat_history = get_mutable_chat_history(&mut data, command.channel_id.get()).await;
        set_system_prompt(chat_history, prompt);
    }
    reply_to_command(&ctx, command, "System prompt set!").await;
}

async fn regenerate(ctx: Context, command: &CommandInteraction) {
    let response = {
        let mut data = ctx.data.write().await;
        let chat_history = get_mutable_chat_history(&mut data, command.channel_id.get()).await;
        if chat_history.len() > 1 {
            chat_history.pop();
        }
        get_llm_response(chat_history).await
    };
    let _ = command.channel_id.say(&ctx.http, response).await;
}

async fn reply_to_command(ctx: &Context, command: &CommandInteraction, message: &str) {
    let _ = command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(message)
                    .ephemeral(true),
            ),
        )
        .await;
}

pub fn register_commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("register")
            .description("Make the bot respond to the registered channel"),
        CreateCommand::new("unregister").description("Remove the bot from the channel"),
        CreateCommand::new("amnesia").description("Reset the chat bots chat history"),
        CreateCommand::new("setprompt")
            .description("Set a new prompt for the channe")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "prompt",
                    "The new system prompt for the bot",
                )
                .required(true),
            ),
        CreateCommand::new("nuke").description("Remove the bots own messages"),
        CreateCommand::new("supernuke").description("Remove 100 messages from the current channel"),
        CreateCommand::new("regenerate").description("Generate a new answer to the latest message"),
    ]
}
