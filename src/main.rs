use dotenv::dotenv;
use lazy_static::lazy_static;
use rand::Rng;
use serenity::futures::future::join_all;
use serenity::http::Http;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
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
        prelude::{ChannelId, GuildChannel},
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
                let mut errors = vec![]; // Vec<(Content, Ephemeral)>
                let newest_message; // (RawMessage, Content, Ephemeral)
                newest_message = match ctx.cache.guild(guild_id) {
                    Some(g) => match g.channels(&ctx.http).await {
                        Ok(channels) => {
                            let channels: Vec<((ChannelId, GuildChannel), Arc<Http>)> = channels
                                .iter()
                                .map(|c| ((c.0.clone(), c.1.clone()), ctx.http.clone()))
                                .collect();
                            let mut messages = vec![]; // Vec<(RawMessage, Content, Ephemeral)>
                            let mut message_async_handles = vec![];
                            for ((channel_id, channel), http) in channels {
                                let handle = tokio::spawn(async move {
                                    let channel_id = channel_id.clone();
                                    let channel = channel.clone();

                                    let dote_role = match GUILD_DOTE_ROLES.get(&guild_id) {
                                        Some(role) => format!("<@&{}>", role),
                                        None => {
                                            eprintln!(
                                                "Could not find dote role for guild {}",
                                                guild_id
                                            );
                                            return None;
                                        }
                                    };

                                    let messages = match channel
                                        .messages(&http, |retriever| retriever.limit(100))
                                        .await
                                    {
                                        Ok(messages) => messages,
                                        Err(e) => {
                                            eprintln!("Error getting messages for channel {:?} in guild {:?} with error {:?}", channel_id, guild_id, e);
                                            return None;
                                        }
                                    };
                                    for message in messages {
                                        if message.content.contains(&dote_role) {
                                            let timestamp = message.timestamp.unix_timestamp();
                                            let elapsed =
                                                Utc::now().naive_utc().signed_duration_since(
                                                    NaiveDateTime::from_timestamp(timestamp, 0),
                                                );
                                            let content = format!(
                                                "Last @Dote message was at <t:{}:T> ({:.02} days ago, from user <@{}>)",
                                                timestamp,
                                                (elapsed.num_seconds() as f64) / (60.0 * 60.0 * 24.0),
                                                message.author.id
                                            );
                                            let found_message = Some(message.to_owned());
                                            return Some((found_message, content, false));
                                        }
                                    }
                                    None
                                });
                                message_async_handles.push(handle);
                            }

                            join_all(message_async_handles)
                                .await
                                .iter()
                                .filter_map(|handle_result| match handle_result {
                                    Ok(result) => result.clone(),
                                    Err(e) => {
                                        eprintln!("Error while joining message handle: {:?}", e);
                                        None
                                    }
                                })
                                .for_each(|(message, content, ephemeral)| {
                                    if let Some(message) = message {
                                        messages.push((message, content, ephemeral));
                                    } else {
                                        errors.push((content, ephemeral));
                                    }
                                });

                            match messages
                                .iter()
                                .max_by(|(message_a, _, _), (message_b, _, _)| {
                                    message_a
                                        .timestamp
                                        .unix_timestamp()
                                        .cmp(&message_b.timestamp.unix_timestamp())
                                }) {
                                Some(message) => {
                                    Some((message.0.clone(), message.1.clone(), message.2))
                                }
                                None => {
                                    let content = format!(
                                        "Failed to find newest message for guild {:?}",
                                        guild_id
                                    );
                                    eprintln!("{}", content);
                                    errors.push((content, true));
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            let content = format!("Error getting channels: {:?}", e);
                            let ephemeral = true;
                            eprintln!("{}", content);
                            errors.push((content, ephemeral));
                            None
                        }
                    },
                    None => {
                        eprintln!("Failed to find guild with id {}", guild_id);
                        return;
                    }
                };

                match newest_message {
                    Some((ref message, ref content, ephemeral)) => {
                        let color: i32 = rand::thread_rng().gen_range(0x000000..=0xffffff);
                        let http = ctx.http.clone();

                        match command
                            .create_interaction_response(http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .embed(|embed| {
                                                embed
                                                    .title("Last @Dote")
                                                    .description(content)
                                                    .color(color)
                                            })
                                            .ephemeral(ephemeral)
                                    })
                            })
                            .await
                        {
                            Ok(_) => match message.reply(&ctx.http, "Here").await {
                                Ok(_) => (),
                                Err(e) => eprintln!("Error replying to message: {:?}", e),
                            },
                            Err(e) => eprintln!("Error adding interaction response: {:?}", e),
                        }
                    }
                    None => {
                        let error = match errors.first() {
                            Some((content, _)) => content.clone(),
                            None => String::from("Unknown Error Occured"),
                        };
                        let color: i32 = rand::thread_rng().gen_range(0x000000..=0xffffff);
                        match command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .embed(|embed| {
                                                embed.title("Error").description(error).color(color)
                                            })
                                            .ephemeral(true)
                                    })
                            })
                            .await
                        {
                            Ok(_) => (),
                            Err(e) => eprintln!("Error adding interaction response: {:?}", e),
                        }
                    }
                }

                if errors.len() > 0 {
                    if newest_message.is_some() {
                        match command
                            .create_followup_message(&ctx.http, |message| {
                                message
                                    .embed(|embed| {
                                        embed
                                            .title("Followup Errors")
                                            .description(format!(
                                                "There were errors:\n{}",
                                                errors
                                                    .iter()
                                                    .map(|(content, _)| format!("\t{}", content))
                                                    .collect::<Vec<String>>()
                                                    .join("\n")
                                            ))
                                            .color(0xFF0000)
                                    })
                                    .ephemeral(true)
                            })
                            .await
                        {
                            Ok(_) => (),
                            Err(e) => eprintln!("Error adding followup message: {:?}", e),
                        }
                    } else {
                        match command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .embed(|embed| {
                                                embed
                                                    .title("Followup Errors")
                                                    .description(format!(
                                                        "There were errors:\n{}",
                                                        errors
                                                            .iter()
                                                            .map(|(content, _)| format!(
                                                                "\t{}",
                                                                content
                                                            ))
                                                            .collect::<Vec<String>>()
                                                            .join("\n")
                                                    ))
                                                    .color(0xFF0000)
                                            })
                                            .ephemeral(true)
                                    })
                            })
                            .await
                        {
                            Ok(_) => (),
                            Err(e) => eprintln!("Error sending error message: {:?}", e),
                        }
                    }
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
