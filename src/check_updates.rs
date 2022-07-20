use chrono::{DateTime, TimeZone, Utc};
use lazy_static::lazy_static;
use rand::Rng;
use serde_json::Value;
use serenity::futures::lock::Mutex;
use serenity::model::id::ChannelId;
use serenity::{model::id::GuildId, CacheAndHttp};
use std::{collections::HashMap, sync::Arc};

const SLEEP_TIME: u64 = 60;

lazy_static! {
    static ref NEWEST: Arc<Mutex<(DateTime<Utc>, String)>> = Arc::new(Mutex::new((Utc::now(), String::default()))); // (time, gid)
    static ref CHANNELS: Arc<Mutex<HashMap<GuildId, [u64; 2]>>> = {
        let hash : HashMap<GuildId, [u64; 2]> = [
            (GuildId::from(434511133383065620u64), [999205229067259934, 999205213783208016]),
            (GuildId::from(983098809733226577), [983098809733226580, 999215240464052294]),
            ].iter().map(|val| val.to_owned()).collect(); // (gid, [ping, updates])
        Arc::new(Mutex::new(hash))
    };
}

pub async fn check_updates(cache_and_http: &Arc<CacheAndHttp>) {
    while !crate::READY.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    loop {
        let initial_request = match reqwest::get("http://api.steampowered.com/ISteamNews/GetNewsForApp/v0002/?appid=570&count=10&maxlength=300&format=json").await {
                Ok(response) => response,
                Err(e) => {
                    eprintln!("Error checking for updates: {:?}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                    continue;
                }
            };
        let response_body = match initial_request.text().await {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Failed to get body of response from reqwest: {:?}", e);
                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                continue;
            }
        };
        let json: Value = match serde_json::from_str(&response_body) {
            Ok(json) => json,
            Err(e) => {
                eprintln!(
                    "Failed to convert response from Valve API with error: {:?}",
                    e
                );
                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                continue;
            }
        };

        let appnews = match json.get("appnews") {
            Some(appnews) => appnews,
            None => {
                eprintln!("No appnews found in response from Valve API");
                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                continue;
            }
        };

        let newsitems = match appnews.get("newsitems") {
            Some(Value::Array(newsitems)) => newsitems,
            Some(v) => {
                eprintln!("Response from Valve API had property newsitems being something other than an array... {}", v);
                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                continue;
            }
            None => {
                eprintln!("No newsitems found in response from Valve API");
                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
                continue;
            }
        };

        struct Article {
            pub title: String,
            pub author: String,
            pub url: String,
            pub date: DateTime<Utc>,
        }

        let mut updated: Option<Article> = None;

        for item in newsitems {
            let tags_raw = match item.get("tags") {
                Some(Value::Array(tags)) => tags,
                Some(v) => {
                    eprintln!("Response from Valve API had property tags being something other than an array... {}", v);
                    continue;
                }
                None => {
                    continue;
                }
            };
            let found_patchnotes = tags_raw.iter().find_map(|tag| match tag {
                Value::String(inner_tag) if inner_tag == "patchnotes" => Some(()),
                _ => None,
            });
            if found_patchnotes.is_none() {
                continue;
            }

            let item_date = match item.get("date") {
                Some(Value::Number(date)) => match date.as_i64() {
                    Some(date) => Utc.timestamp(date, 0),
                    None => {
                        eprintln!(
                            "Failed to convert date for newsitem entry from Valve API to i64"
                        );
                        continue;
                    }
                },
                Some(v) => {
                    eprintln!("newsitem entry from Valve API had property date being something other than a number... {}", v);
                    continue;
                }
                None => {
                    eprintln!("No date found for newsitem entry in response from Valve API");
                    continue;
                }
            };

            macro_rules! get_string {
                ($item: expr, $property: expr) => {
                    match $item.get($property) {
                        Some(Value::String(string)) => string.to_string(),
                        Some(v) => {
                            eprintln!(
                                "newsitem entry from Valve API had property {} being something other than a string... {}",
                                $property, v
                            );
                            continue;
                        }
                        None => {
                            eprintln!("No {} found for newsitem entry in response from Valve API", $property);
                            continue;
                        }
                    }
                };
            }

            let item_gid = get_string!(item, "gid");
            let item_title = get_string!(item, "title");
            let item_author = get_string!(item, "author");
            let item_url = get_string!(item, "url");

            let mut newest = NEWEST.lock().await;
            if item_date >= newest.0 && item_gid != newest.1 {
                println!("New update found: {}", item_title);
                newest.0 = item_date;
                newest.1 = item_gid;
                updated = Some(Article {
                    title: item_title,
                    author: item_author,
                    url: item_url,
                    date: item_date,
                });
            }
        }

        if let Some(article) = updated {
            let players = get_users_playing(cache_and_http).await;
            for (guild_id, guild_players) in players.into_iter() {
                let channels_raw: [u64; 2] = {
                    let channels_lock = CHANNELS.lock().await;
                    match channels_lock.get(&guild_id) {
                        Some(channels) => channels.to_owned(),
                        None => {
                            eprintln!("No channels found for guild {}", guild_id);
                            continue;
                        }
                    }
                };

                let cache = &cache_and_http.cache;
                let http = &cache_and_http.http;

                let channel_map = match cache.guild_channels(guild_id) {
                    Some(channel_map) => channel_map,
                    None => {
                        eprintln!(
                            "Failed to get references to channels for guild {}",
                            guild_id
                        );
                        continue;
                    }
                };
                let ping_channel = match channel_map.get(&ChannelId::from(channels_raw[0])) {
                    Some(channel) => channel,
                    None => {
                        eprintln!(
                            "No ping channel found for guild {} (should be under ID {})",
                            guild_id, channels_raw[0]
                        );
                        continue;
                    }
                };
                let updates_channel = match channel_map.get(&ChannelId::from(channels_raw[1])) {
                    Some(channel) => channel,
                    None => {
                        eprintln!(
                            "No updates channel found for guild {} (should be under ID {})",
                            guild_id, channels_raw[1]
                        );
                        continue;
                    }
                };

                match guild_players.len() {
                    0 => (),
                    1 => {
                        match ping_channel
                            .say(
                                http,
                                format!(
                                    "Attention <@{}> : You need to restart dota. There is an update!",
                                    guild_players[0].id
                                ),
                            )
                            .await
                        {
                            Ok(_) => (),
                            Err(e) => {
                                eprintln!("Failed to send message to ping channel: {}", e);
                            }
                        };
                    }
                    2 => {
                        match ping_channel
                            .say(
                                http,
                                format!(
                                    "Attention <@{}> and <@{}> : You need to restart dota. There is an update!",
                                    guild_players[0].id,
                                    guild_players[1].id
                                ),
                            )
                            .await
                        {
                            Ok(_) => (),
                            Err(e) => {
                                eprintln!("Failed to send message to ping channel: {}", e);
                            }
                        };
                    }
                    _ => {
                        let mut players_string = String::from("Attention ");
                        for player in &guild_players[..guild_players.len() - 1] {
                            players_string.push_str(&format!("<@{}> ,", player.id));
                        }
                        players_string.push_str(&format!(
                            "and <@{}> : You need to restart dota. There is an update!",
                            guild_players[guild_players.len() - 1].id
                        ));
                        match ping_channel.say(http, players_string).await {
                            Ok(_) => {
                                println!("Sent message to ping channel for guild {}", guild_id);
                            }
                            Err(e) => {
                                eprintln!("Failed to send message to ping channel: {}", e);
                            }
                        };
                    }
                }

                match updates_channel
                    .send_message(http, |m| {
                        m.embed(|e| {
                            e.title(&article.title);
                            e.author(|a| a.name(&article.author));
                            e.description(format!("A new update is available! Please see [here]({}) for more information.", article.url));
                            e.timestamp(article.date.to_rfc3339());
                            e.color(rand::thread_rng().gen_range(0x000000..=0xffffff));
                            e
                        })
                    })
                    .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Failed to send message to updates channel: {}", e);
                    }
                };
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(SLEEP_TIME)).await;
    }
}

pub async fn get_users_playing(
    cache_and_http: &Arc<CacheAndHttp>,
) -> HashMap<GuildId, Vec<serenity::model::user::User>> {
    let cache = &cache_and_http.cache;
    let guilds = cache.guilds();
    let returnable: HashMap<GuildId, Vec<serenity::model::user::User>> = guilds
        .iter()
        .filter_map(|guild_id| {
            let guild = match cache.guild(guild_id) {
                Some(g) => g,
                None => {
                    eprintln!(
                        "Failed to get guild with id {} when checking presences.",
                        guild_id.0
                    );
                    return None;
                }
            };

            let presences = guild.presences;
            let users_playing: Vec<serenity::model::user::User> = presences
                .into_iter()
                .filter_map(|(user_id, presence)| {
                    let user = match cache.user(user_id) {
                        Some(u) => u,
                        None => {
                            eprintln!(
                                "Failed to get user with id {} when checking presence.",
                                user_id
                            );
                            return None;
                        }
                    };
                    if !user.bot {
                        let activities = presence.activities;
                        for activity in activities {
                            if activity.name.to_ascii_lowercase().contains("dota") {
                                return Some(user);
                            }
                        }
                    }
                    None
                })
                .collect();
            Some((guild_id.clone(), users_playing))
        })
        .collect();
    returnable
}
