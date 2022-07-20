use dotenv::dotenv;
use lazy_static::lazy_static;
use std::env;
use std::sync::atomic::AtomicBool;

use serenity::async_trait;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

mod check_updates;
use check_updates::check_updates;

struct Handler;

lazy_static! {
    static ref READY: AtomicBool = AtomicBool::new(false);
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.tag());
        READY.store(true, std::sync::atomic::Ordering::Relaxed);
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
