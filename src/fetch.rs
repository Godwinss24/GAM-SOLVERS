use std::collections::HashMap;

use crate::parse;

pub use crate::parse::{Data, Detail};

/// Fetch a GAMS solver manual and parse both the summary option table and the
/// per-option detailed entries (declared types and enumerated string values).
pub async fn scrape_gams_solvers(
    url: &str,
) -> Result<(Vec<Data>, HashMap<String, Detail>), reqwest::Error> {
    let response = reqwest::get(url).await?;
    let html = response.text().await?;
    Ok((parse::parse_solver_options(&html), parse::parse_option_details(&html)))
}
