use crate::{
    modules::lorax::database::{LoraxEvent, LoraxStage},
    Context, Error,
};
use poise::{
    command,
    serenity_prelude::{
        self as serenity, ButtonStyle, ComponentInteractionDataKind, CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption
    },
    CreateReply,
};
use tracing::{error, info};

const RESERVED_TREES: [&str; 10] = [
    "maple", "sakura", "baobab", "sequoia", "oak", "pine", "palm", "willow", "cherry", "redwood",
];

async fn fetch_node_names() -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://metrics.pyro.host/api/v1/query")
        .query(&[("query", "node_uname_info")])
        .send()
        .await
        .map_err(|e| format!("Failed to fetch metrics: {}", e))?;

    #[derive(serde::Deserialize)]
    struct PrometheusResponse {
        data: Data,
    }

    #[derive(serde::Deserialize)]
    struct Data {
        result: Vec<Result>,
    }

    #[derive(serde::Deserialize)]
    struct Result {
        metric: Metric,
    }

    #[derive(serde::Deserialize)]
    struct Metric {
        nodename: String,
    }

    let data: PrometheusResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(data
        .data
        .result
        .into_iter()
        .map(|r| r.metric.nodename.to_lowercase())
        .collect())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn submit(
    ctx: Context<'_>,
    #[description = "Your awesome tree name"] name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx.guild_id().unwrap().get();
    let user_id = ctx.author().id.get();

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("🛑 Oops! There's no Lorax event happening right now.")
                .await?;
            return Ok(());
        }
    };

    if event.stage == LoraxStage::Voting {
        ctx.say("🗳️ Submission period has ended, but voting is open!\n💡 Use `/lorax vote` to pick your favorite tree name.").await?;
        return Ok(());
    }

    if event.stage != LoraxStage::Submission {
        ctx.say("🚫 Submissions are closed at the moment. Stay tuned for the next event!")
            .await?;
        return Ok(());
    }

    let name = name.to_lowercase().trim().to_string();

    if !is_appropriate_name(&name) {
        ctx.say("❌ Invalid tree name. Please ensure that the name is appropriate!")
            .await?;

        info!(
            "Inappropriate name \"{}\" submitted by {}",
            name,
            ctx.author().tag()
        );
        return Ok(());
    }

    if !is_valid_tree_name(&name) {
        ctx.say(
            "❌ Invalid tree name. Please ensure it is between 3 and 32 alphabetic characters.",
        )
        .await?;
        return Ok(());
    }

    match fetch_node_names().await {
        Ok(node_names) => {
            if node_names.contains(&name) {
                ctx.say(
                    "🌲 That tree name is already in use as a node name. Please choose another!",
                )
                .await?;
                return Ok(());
            }
        }
        Err(e) => {
            error!("Failed to fetch node names: {}", e);
        }
    }

    if RESERVED_TREES.contains(&name.as_str()) || name == "lorax" {
        ctx.say("🌲 That tree name is reserved. Try coming up with something unique! 🍃")
            .await?;
        return Ok(());
    }

    if event.tree_submissions.values().any(|t| t == &name) {
        ctx.say("🌳 Someone already suggested that name! How about a different one?")
            .await?;
        return Ok(());
    }

    match ctx
        .data()
        .dbs
        .lorax
        .submit_tree(guild_id, name.clone(), user_id)
        .await
    {
        Ok((is_update, old_submission)) => {
            let msg = if is_update {
                format!(
                    "🔄 Updated your submission from \"**{}**\" to \"**{}**\"!\n⏳ Stay tuned for the voting phase.",
                    old_submission.unwrap_or_default(),
                    name
                )
            } else {
                format!(
                    "🌳 Your tree name \"**{}**\" has been submitted!\n⏳ Stay tuned for the voting phase.",
                    name
                )
            };
            ctx.say(msg).await?;
        }
        Err(e) => {
            ctx.say(format!("❌ Unable to submit: {}", e)).await?;
        }
    }

    Ok(())
}

const FORBIDDEN_LIST: &str = include_str!("../../../../extra/banned_words.txt");

fn is_appropriate_name(name: &str) -> bool {
    let name = name.to_lowercase();
    let words: Vec<&str> = name.split_whitespace().collect();

    for forbidden in FORBIDDEN_LIST.lines() {
        let forbidden = forbidden.trim().to_lowercase();
        if forbidden.is_empty() {
            continue;
        }

        for word in &words {
            if *word == forbidden {
                return false;
            }
        }
    }
    true
}

fn is_valid_tree_name(name: &str) -> bool {
    let name = name.trim();
    let length = name.len();
    (3..=32).contains(&length) && name.chars().all(|c| c.is_ascii_alphabetic())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn vote(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx.guild_id().unwrap().get();
    let user_id = ctx.author().id.get();

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("❌ There is no active event at the moment!")
                .await?;
            return Ok(());
        }
    };

    if !is_voting_stage(&event.stage) {
        ctx.say("🚫 Voting is not active at the moment.").await?;
        return Ok(());
    }

    let mut trees = get_available_trees(&event, user_id);
    if trees.is_empty() {
        ctx.say("🤔 There's nothing to vote on yet. Wait for more submissions!")
            .await?;
        return Ok(());
    }

    trees.sort();

    let page_size = 25;
    let total_pages = (trees.len() as f32 / page_size as f32).ceil() as usize;
    let mut current_page = 0;

    let create_reply = |page: usize| {
        let mut components = vec![CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                "vote_tree",
                CreateSelectMenuKind::String {
                    options: trees
                        [page * page_size..(page * page_size + page_size).min(trees.len())]
                        .iter()
                        .map(|tree| CreateSelectMenuOption::new(tree, tree))
                        .collect(),
                },
            )
            .placeholder("Choose wisely..."),
        )];

        if total_pages > 1 {
            components.push(CreateActionRow::Buttons(vec![
                CreateButton::new("prev_page")
                    .emoji('◀')
                    .style(ButtonStyle::Secondary)
                    .disabled(page == 0),
                CreateButton::new("next_page")
                    .emoji('▶')
                    .style(ButtonStyle::Secondary)
                    .disabled(page >= total_pages - 1),
            ]));
        }

        CreateReply::default()
            .content(format!(
                "🗳️ **Vote for your favorite tree name!** (Page {}/{})\nNote: You can't vote for your own.",
                page + 1,
                total_pages
            ))
            .components(components)
    };

    let create_update = |page: usize| {
        let mut components = vec![CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                "vote_tree",
                CreateSelectMenuKind::String {
                    options: trees
                        [page * page_size..(page * page_size + page_size).min(trees.len())]
                        .iter()
                        .map(|tree| CreateSelectMenuOption::new(tree, tree))
                        .collect(),
                },
            )
            .placeholder("Choose wisely..."),
        )];

        if total_pages > 1 {
            components.push(CreateActionRow::Buttons(vec![
                CreateButton::new("prev_page")
                    .emoji('◀')
                    .style(ButtonStyle::Secondary)
                    .disabled(page == 0),
                CreateButton::new("next_page")
                    .emoji('▶')
                    .style(ButtonStyle::Secondary)
                    .disabled(page >= total_pages - 1),
            ]));
        }

        CreateInteractionResponseMessage::new()
            .content(format!(
                "🗳️ Pick your favorite tree name: (Page {}/{})\nNote: You can't vote for your own.",
                page + 1,
                total_pages
            ))
            .components(components)
    };

    let msg = ctx.send(create_reply(current_page)).await?;

    while let Some(interaction) = msg
        .message()
        .await?
        .await_component_interaction(ctx)
        .author_id(ctx.author().id)
        .timeout(std::time::Duration::from_secs(60))
        .await
    {
        match interaction.data.custom_id.as_str() {
            "prev_page" => {
                if current_page > 0 {
                    current_page -= 1;
                    interaction
                        .create_response(
                            &ctx.serenity_context().http,
                            CreateInteractionResponse::UpdateMessage(create_update(current_page)),
                        )
                        .await?;
                }
            }
            "next_page" => {
                if current_page < total_pages - 1 {
                    current_page += 1;
                    interaction
                        .create_response(
                            &ctx.serenity_context().http,
                            CreateInteractionResponse::UpdateMessage(create_update(current_page)),
                        )
                        .await?;
                }
            }
            "vote_tree" => {
                if let ComponentInteractionDataKind::StringSelect { values, .. } =
                    &interaction.data.kind
                {
                    handle_vote_selection(&ctx, &interaction, values, guild_id, user_id).await?;
                    return Ok(());
                }
            }
            _ => return Err("Unexpected event type id".into()),
        }
    }

    ctx.say("⌛ Time's up! Feel free to `/lorax vote` again anytime.")
        .await?;
    Ok(())
}

/// Checks if the given tree name is taken by a node in the cluster.
#[command(slash_command, guild_only, ephemeral)]
pub async fn check(
    ctx: Context<'_>,
    #[description = "Tree name to check"] name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let name = name.to_lowercase().trim().to_string();

    if !is_valid_tree_name(&name) {
        ctx.say(
            "❌ Invalid tree name. Please ensure it is between 3 and 32 alphabetic characters.",
        )
        .await?;
        return Ok(());
    }

    match fetch_node_names().await {
        Ok(node_names) => {
            if node_names.contains(&name) {
                ctx.say(
                    "🌲 That tree name is already in use as a node name. Please choose another!",
                )
                .await?;
            } else {
                ctx.say("🌳 That tree name is available! Go ahead and submit it.")
                    .await?;
            }
        }
        Err(e) => {
            error!("Failed to fetch node names: {}", e);
            ctx.say("❌ Failed to fetch node names. Please try again later.")
                .await?;
        }
    }

    Ok(())
}

fn is_voting_stage(stage: &LoraxStage) -> bool {
    matches!(stage, LoraxStage::Voting | LoraxStage::Tiebreaker(_))
}

fn get_available_trees(event: &LoraxEvent, _user_id: u64) -> Vec<String> {
    event
        .tree_submissions
        .iter()
        .map(|(_, tree)| tree.clone())
        .collect()
}

async fn handle_vote_selection(
    ctx: &Context<'_>,
    interaction: &serenity::ComponentInteraction,
    values: &[String],
    guild_id: u64,
    user_id: u64,
) -> Result<(), Error> {
    let selected_tree = values.first().ok_or("No selection made")?;

    // Check if user is trying to vote for their own submission
    let event = ctx.data().dbs.lorax.get_event(guild_id).await.unwrap();
    if let Some(submitter_id) = event.get_tree_submitter(selected_tree) {
        if submitter_id == user_id {
            interaction
                .create_response(
                    &ctx.serenity_context().http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("❌ You cannot vote for your own submission!")
                            .components(vec![]),
                    ),
                )
                .await?;
            return Ok(());
        }
    }

    match ctx
        .data()
        .dbs
        .lorax
        .transaction(|db| {
            let event = db
                .events
                .get_mut(&guild_id)
                .ok_or_else(|| "No active event".to_string())?;

            let old_vote = event.tree_votes.insert(user_id, selected_tree.to_string());

            if let Some(old) = old_vote {
                Ok(format!(
                    "Changed vote from \"{}\" to \"{}\"",
                    old, selected_tree
                ))
            } else {
                Ok("Vote recorded!".to_string())
            }
        })
        .await
    {
        Ok(msg) => {
            interaction
                .create_response(
                    &ctx.serenity_context().http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(format!("✅ {}", msg))
                            .components(vec![]),
                    ),
                )
                .await?;
        }
        Err(e) => {
            interaction
                .create_response(
                    &ctx.serenity_context().http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(format!("❌ Unable to cast vote: {}", e))
                            .components(vec![]),
                    ),
                )
                .await?;
        }
    }

    Ok(())
}
