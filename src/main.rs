use dotenv::dotenv;
use lazy_static::lazy_static;
use rand::Rng;
use std::sync::atomic::AtomicBool;
use std::{collections::HashMap, env};

use serenity::{
    async_trait,
    model::{
        application::{
            command::Command,
            interaction::{Interaction, InteractionResponseType},
        },
        gateway::Ready,
        id::GuildId,
    },
    prelude::*,
};

use chrono::{NaiveDateTime, Utc};

mod check_updates;
use check_updates::check_updates;

struct Handler;

lazy_static! {
    static ref READY: AtomicBool = AtomicBool::new(false);
    static ref GUILD_DOTE_ROLES: HashMap<GuildId, u64> = [
        (GuildId::from(434511133383065620u64), 1005581009569460305),
        (GuildId::from(983098809733226577), 983217658575081522),
    ]
    .iter()
    .map(|val| val.to_owned())
    .collect();
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.tag());

        match Command::create_global_application_command(&ctx.http, |command| {
            command
                .name("lastping")
                .description("Displays the last time someone pinged for @Dote")
        })
        .await
        {
            Ok(c) => {
                println!("Added command: {:#?}", c);
            }
            Err(e) => eprintln!("Error adding command: {:?}", e),
        }

        READY.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let guild_id = match command.guild_id {
                Some(guild_id) => guild_id,
                None => {
                    eprintln!("Command was not sent in a guild");
                    return;
                }
            };
            if command.data.name == "lastping" {
                let mut content = String::new();
                let mut ephemeral = false;
                let mut found_message = None;
                let dote_role = match GUILD_DOTE_ROLES.get(&guild_id) {
                    Some(role) => format!("<@&{}>", role),
                    None => {
                        eprintln!("Could not find dote role for guild {}", guild_id);
                        return;
                    }
                };
                match ctx.cache.guild(guild_id) {
                    Some(g) => match g.channels(&ctx.http).await {
                        Ok(channels) => {
                            'top: for (channel_id, channel) in channels {
                                let messages = match channel
                                    .messages(&ctx.http, |retriever| retriever.limit(100))
                                    .await
                                {
                                    Ok(messages) => messages,
                                    Err(e) => {
                                        eprintln!("Error getting messages for channel {:?} in guild {:?} with error {:?}", channel_id, guild_id, e);
                                        continue;
                                    }
                                };
                                for message in messages {
                                    if message.content.contains(&dote_role) {
                                        let timestamp = message.timestamp.unix_timestamp();
                                        let elapsed = Utc::now().naive_utc().signed_duration_since(
                                            NaiveDateTime::from_timestamp(timestamp, 0),
                                        );
                                        content = format!(
                                            "Last @Dote message was at <t:{}:T> ({:.02} days ago, from user <@{}>)",
                                            timestamp,
                                            (elapsed.num_seconds() as f64) / (60.0 * 60.0 * 24.0),
                                            message.author.id
                                        );
                                        found_message = Some(message);
                                        break 'top;
                                    }
                                }
                            }
                            if found_message.is_none() && content.len() == 0 {
                                content =
                                    format!("No @Dote messages found in guild {:?}", guild_id);
                                ephemeral = true;
                            }
                        }
                        Err(e) => {
                            content = format!("Error getting channels: {:?}", e);
                            ephemeral = true;
                            eprintln!("{}", content);
                        }
                    },
                    None => {
                        eprintln!("Failed to find guild with id {}", guild_id);
                        return;
                    }
                }

                match command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .embed(|embed| {
                                        embed
                                            .title(if ephemeral { "Error" } else { "Last @Dote" })
                                            .description(content)
                                            .color(
                                                rand::thread_rng().gen_range(0x000000..=0xffffff),
                                            )
                                    })
                                    .ephemeral(ephemeral)
                            })
                    })
                    .await
                {
                    Ok(_) => {
                        if let Some(message) = found_message {
                            match message.reply(&ctx.http, "Here").await {
                                Ok(_) => (),
                                Err(e) => eprintln!("Error replying to message: {:?}", e),
                            }
                        }
                    }
                    Err(e) => eprintln!("Error adding interaction response: {:?}", e),
                }
            }
        }
    }
}
#[tokio::main]
async fn main() {
    // Login with a bot token from the environment
    dotenv().ok();
    let token = env::var("DISCORD_TOKEN").expect("token");
    let intents = GatewayIntents::privileged() | GatewayIntents::non_privileged();
    let mut client = Client::builder(token, intents)
        .event_handler(Handler)
        .await
        .expect("Error creating client");

    let cache_and_http = client.cache_and_http.clone();
    let _update_loop = tokio::spawn(async move {
        let cache_and_http = cache_and_http;
        check_updates(&cache_and_http).await
    });

    // start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}
