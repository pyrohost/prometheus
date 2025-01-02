use chrono::{Datelike, NaiveDate, Utc};
use std::collections::HashMap;
use crate::{Context, Error};

#[derive(Debug)]
struct ServerEntry {
    name: String,
    hostname: String,
    price: f64,
    date: NaiveDate,
    location: String,
    cpu_model: String,
    payment_period: String,  // Add this new field
}

impl ServerEntry {
    fn parse(block: &str) -> Option<Self> {
        let lines: Vec<&str> = block.lines().collect();
        if lines.len() < 3 {
            return None;
        }

        let name = lines[0].trim().to_string();
        
        // Parse CPU model
        let cpu_model = name
            .split('-')
            .nth(1)?
            .trim()
            .split("192GB")
            .next()?
            .trim()
            .to_string();

        // Parse hostname and price
        let hostname_line = lines[1].trim();
        let hostname = hostname_line.split_whitespace().next()?.to_string();
        let price = hostname_line
            .split('$')
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()?;

        // Parse date with payment period determination
        let date_str = lines[2].split_whitespace().nth(1)?;
        let date = NaiveDate::parse_from_str(date_str, "%m/%d/%Y").ok()?;
        
        let now = Utc::now().naive_utc().date();
        let payment_period = if date.month() == now.month() && date.year() == now.year() {
            "Current Month".to_string()
        } else if date > now {
            "Future".to_string()
        } else {
            "Past".to_string()
        };

        // Parse location
        let location = if hostname.contains("mia") {
            "Miami"
        } else if hostname.contains("lax") {
            "Los Angeles"
        } else if hostname.contains("nyc") {
            "New York"
        } else {
            "Other"
        }.to_string();

        Some(ServerEntry {
            name,
            hostname,
            price,
            date,
            location,
            cpu_model,
            payment_period,
        })
    }
}

/// Analyzes server costs and provides a detailed breakdown
/// 
/// This command calculates:
/// - Total costs for the current payment period
/// - Location-based cost distribution
/// - CPU model distribution
/// - Payment schedule analysis
/// 
/// The analysis excludes servers with due dates outside the current payment period.
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR", ephemeral)]
pub async fn server_costs<'a>(
    ctx: Context<'a>,
    #[description = "Server list (paste the full list)"] input: String,
) -> Result<(), Error> {
    let current_month = Utc::now().month();
    let current_year = Utc::now().year();

    let servers: Vec<ServerEntry> = input
        .split("\n\n")
        .filter_map(ServerEntry::parse)
        .collect();

    if servers.is_empty() {
        ctx.say("❌ No valid server entries found in input.").await?;
        return Ok(());
    }

    let current_servers: Vec<&ServerEntry> = servers
        .iter()
        .filter(|s| s.date.month() == current_month && s.date.year() == current_year)
        .collect();

    if current_servers.is_empty() {
        ctx.say("❌ No servers due for payment in the current period.").await?;
        return Ok(());
    }

    let total_cost: f64 = current_servers.iter().map(|s| s.price).sum();
    let mut location_costs: HashMap<String, (i32, f64)> = HashMap::new();
    let mut cpu_counts: HashMap<String, i32> = HashMap::new();

    // Calculate statistics
    for server in &current_servers {
        let entry = location_costs
            .entry(server.location.clone())
            .or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += server.price;

        *cpu_counts.entry(server.cpu_model.clone()).or_insert(0) += 1;
    }

    // Build response
    let mut response = format!("🔒 **Server Cost Analysis for {}/{}**\n\n", current_month, current_year);
    
    response.push_str("**Payment Period Breakdown:**\n");
    response.push_str(&format!("• Due this month: {} servers\n", current_servers.len()));
    response.push_str(&format!("• Total servers: {}\n\n", servers.len()));

    response.push_str("**Location Distribution:**\n");
    for (location, (count, cost)) in &location_costs {
        let percentage = (count * 100) as f64 / current_servers.len() as f64;
        response.push_str(&format!(
            "• {} - {} servers ({:.1}%): ${:.2} USD\n",
            location, count, percentage, cost
        ));
    }

    response.push_str("\n**CPU Distribution:**\n");
    for (cpu, count) in &cpu_counts {
        let percentage = (count * 100) as f64 / current_servers.len() as f64;
        response.push_str(&format!("• {} - {} units ({:.1}%)\n", cpu, count, percentage));
    }

    response.push_str("\n**Servers Due This Month:**\n");
    for server in &current_servers {
        response.push_str(&format!(
            "• {} ({}) - ${:.2} USD\n  Payment Due: {}\n  Location: {}\n  Status: {}\n",
            server.name,
            server.hostname,
            server.price,
            server.date.format("%m/%d/%Y"),
            server.location,
            server.payment_period
        ));
    }

    response.push_str(&format!(
        "\n**Financial Summary:**\n• Current Period Total: ${:.2} USD\n• Average Cost per Server: ${:.2} USD",
        total_cost,
        total_cost / current_servers.len() as f64
    ));

    ctx.say(response).await?;
    Ok(())
}