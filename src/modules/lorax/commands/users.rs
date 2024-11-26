use crate::{modules::lorax::database::LoraxStage, Context, Error};
use poise::{
    command,
    CreateReply,
    serenity_prelude::{
        ComponentInteractionDataKind, CreateActionRow, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, ButtonStyle, CreateButton,
    },
};

const RESERVED_TREES: [&str; 10] = [
    "maple",   // for a canada server
    "sakura",  // for a japan server
    "baobab",  // for an africa server
    "sequoia", // for a california server
    "oak",     // for a british server
    "pine",    // for a sweden server
    "palm",    // for a tropical server
    "willow",  // for a magical server
    "cherry",  // for a blossom server
    "redwood", // for a california server
];

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
            ctx.say("🛑 Oops! There's no Lorax event happening right now.").await?;
            return Ok(());
        }
    };

    if event.stage == LoraxStage::Voting {
        ctx.say("🗳️ Submission period has ended, but voting is open!\n💡 Use `/lorax vote` to pick your favorite tree name.").await?;
        return Ok(());
    }

    if event.stage != LoraxStage::Submission {
        ctx.say("🚫 Submissions are closed at the moment. Stay tuned for the next event!").await?;
        return Ok(());
    }

    let name = name.to_lowercase().trim().to_string();

    // validation checks
    if name.len() < 3 || name.len() > 32 {
        ctx.say("❌ Your tree name must be between 3 and 32 characters long.\n💡 Example: \"maple\", \"birch\", \"magnolia\"").await?;
        return Ok(());
    }

    if name.chars().any(|c| !c.is_ascii_alphabetic()) {
        ctx.say("❌ Tree names can only contain letters (a-z).\n💡 Example: \"maple\", \"birch\", \"magnolia\"").await?;
        return Ok(());
    }

    if RESERVED_TREES.contains(&name.as_str()) || name == "lorax" {
        ctx.say("🌲 That tree name is reserved. Try coming up with something unique! 🍃").await?;
        return Ok(());
    }

    if event.tree_submissions.values().any(|t| t == &name) {
        ctx.say("🌳 Someone already suggested that name! How about a different one?").await?;
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

    // Update this check to include Tiebreaker stage
    if !matches!(event.stage, LoraxStage::Voting | LoraxStage::Tiebreaker(_)) {
        let msg = if event.stage == LoraxStage::Submission {
            "💡 Voting hasn't started yet, but you can `/submit` your tree name suggestion!"
        } else {
            "❌ Voting period has ended. Check back for the next event!"
        };
        ctx.say(msg).await?;
        return Ok(());
    }

    let mut trees: Vec<String> = event
        .tree_submissions
        .iter()
        .filter(|(&submitter_id, _)| submitter_id != user_id) // Only filter out own submission
        .map(|(_, tree)| tree.clone())
        .collect();

    if trees.is_empty() {
        ctx.say("🤔 There's nothing to vote on yet. Wait for more submissions!").await?;
        return Ok(());
    }

    trees.sort();

    let page_size = 25;
    let total_pages = (trees.len() as f32 / page_size as f32).ceil() as usize;
    let mut current_page = 0;

    let create_reply = |page: usize| {
        let mut components = vec![
            CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "vote_tree",
                    CreateSelectMenuKind::String {
                        options: trees[page * page_size..(page * page_size + page_size).min(trees.len())]
                            .iter()
                            .map(|tree| CreateSelectMenuOption::new(tree, tree))
                            .collect(),
                    },
                )
                .placeholder("Choose wisely..."),
            ),
        ];

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
                "🗳️ **Vote for your favorite tree name!** (Page {}/{})",
                page + 1,
                total_pages
            ))
            .components(components)
    };

    let create_update = |page: usize| {
        let mut components = vec![
            CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "vote_tree",
                    CreateSelectMenuKind::String {
                        options: trees[page * page_size..(page * page_size + page_size).min(trees.len())]
                            .iter()
                            .map(|tree| CreateSelectMenuOption::new(tree, tree))
                            .collect(),
                    },
                )
                .placeholder("Choose wisely..."),
            ),
        ];

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
                "🗳️ Pick your favorite tree name: (Page {}/{})",
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
                if let ComponentInteractionDataKind::StringSelect { values, .. } = &interaction.data.kind {
                    let selected_tree = values.first().ok_or("No selection made")?;
        
                    match ctx
                        .data()
                        .dbs
                        .lorax
                        .cast_vote(guild_id, selected_tree.to_string(), user_id)
                        .await
                    {
                        Ok(was_update) => {
                            let msg = if was_update {
                                format!(
                                    "🔄 Changed your vote to \"**{}**\"!\n⏳ Results will be announced when voting ends.",
                                    selected_tree
                                )
                            } else {
                                format!(
                                    "🗳️ You voted for \"**{}**\"!\n⏳ Results will be announced when voting ends.",
                                    selected_tree
                                )
                            };
                            interaction
                                .create_response(
                                    &ctx.serenity_context().http,
                                    CreateInteractionResponse::UpdateMessage(
                                        CreateInteractionResponseMessage::new()
                                            .content(msg)
                                            .components(vec![]),
                                    ),
                                )
                                .await?;
                            return Ok(());
                        }
                        Err(e) => {
                            interaction
                                .create_response(
                                    &ctx.serenity_context().http,
                                    CreateInteractionResponse::UpdateMessage(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!("❌ Could not record your vote: {}", e))
                                            .components(vec![]),
                                    ),
                                )
                                .await?;
                            return Ok(());
                        }
                    }
                } else {
                    return Err("Invalid interaction data kind".into());
                }
            }
            _ => {}
        }
    }

    ctx.say("⌛ Time's up! Feel free to `/vote` again anytime.").await?;
    Ok(())
}